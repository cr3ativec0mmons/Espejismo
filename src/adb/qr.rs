//! QR-code–based Wi-Fi pairing worker for ADB.
//!
//! # Protocol
//!
//! 1. We generate a cryptographically random *service name* and *password*.
//! 2. We encode them in a QR payload (`WIFI:T:ADB;S:<name>;P:<pwd>;;`) for
//!    display in the UI.
//! 3. When Android scans the QR it starts advertising
//!    `<name>._adb-tls-pairing._tcp` on mDNS and waits for us to pair.
//! 4. We poll `adb mdns services` until we find that exact instance.
//! 5. We call `adb pair <host>:<port> <password>`.
//! 6. We then find `_adb-tls-connect._tcp` from the same host and call
//!    `adb connect <host>:<connect-port>`.
//!
//! # Security notes
//!
//! - The password is stored in a private field and never logged.
//! - Randomness comes from `/dev/urandom` (Linux only, acceptable for a GTK
//!   desktop application targeting Linux).
//! - The password is not explicitly zeroed on drop; add the `zeroize` crate
//!   if stricter memory hygiene is required.

use std::ffi::OsString;
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::AdbRunner;
use crate::adb::runner::CommandAdbRunner;

const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp";
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp";

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read exactly `buf.len()` cryptographically random bytes from `/dev/urandom`.
fn read_urandom(buf: &mut [u8]) -> io::Result<()> {
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(buf)
}

/// Encode a byte slice as lowercase hexadecimal.
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

// ── QR session ───────────────────────────────────────────────────────────────

/// A pairing session described by its QR payload.
///
/// Create with [`QrSession::generate`]; then:
/// - Render [`QrSession::payload`] as a QR code in the UI.
/// - Pass the session to [`start_pairing_worker`] to begin discovery.
pub struct QrSession {
    /// mDNS instance name that Android will advertise once it scans the QR.
    /// Format: `adb-{16 lowercase hex chars}`.
    pub service_name: String,
    /// Complete WIFI QR payload: `WIFI:T:ADB;S:<name>;P:<pwd>;;`
    pub payload: String,
    /// Pairing password – private, never logged.
    password: String,
}

impl QrSession {
    /// Generate a new session with cryptographically random identifiers.
    ///
    /// # Errors
    /// Returns `Err` if `/dev/urandom` cannot be read.
    pub fn generate() -> io::Result<Self> {
        let mut buf = [0u8; 16]; // 8 B → service-name suffix, 8 B → password
        read_urandom(&mut buf)?;

        let service_name = format!("adb-{}", to_hex(&buf[..8]));
        let password = to_hex(&buf[8..]);
        // Clear the raw bytes from the stack before they fall out of scope.
        buf.fill(0);

        let payload = format!("WIFI:T:ADB;S:{service_name};P:{password};;");
        Ok(Self {
            service_name,
            payload,
            password,
        })
    }
}

// ── Worker events ─────────────────────────────────────────────────────────────

/// Messages sent from the background pairing worker to the UI.
#[derive(Debug)]
pub enum WorkerEvent {
    /// Human-readable status update (safe to display).
    Status(String),
    /// Pairing and `adb connect` both succeeded.
    Paired,
    /// No device scanned the QR within the allowed time.
    TimedOut,
    /// A non-recoverable error occurred (message is safe to display).
    Error(String),
}

// ── mDNS service record ───────────────────────────────────────────────────────

/// A single record from `adb mdns services`.
#[derive(Debug, Clone)]
pub struct MdnsService {
    /// mDNS instance name (without the service type suffix).
    pub instance: String,
    /// Service type, e.g. `_adb-tls-pairing._tcp`.
    pub service_type: String,
    /// Resolved host address.
    pub host: String,
    /// Port number.
    pub port: u16,
}

/// Parse the output of `adb mdns services`.
///
/// Handles common ADB output formats:
///
/// - **Four columns**: `<instance> <type> <host> <port>`
/// - **Three columns**: `<instance> <type> <host>:<port>`
/// - **Two columns** (full instance name): `<instance>.<type> <host>:<port>`
///
/// Lines that do not contain a recognisable ADB TLS service type are silently
/// skipped, as are the header line and blank lines.
pub fn parse_mdns_services(output: &str) -> Vec<MdnsService> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("List of") {
                return None;
            }
            parse_mdns_line(line)
        })
        .collect()
}

fn parse_mdns_line(line: &str) -> Option<MdnsService> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.as_slice() {
        // Four-column format: instance  type  host  port
        [instance, svc_type, host, port] if is_adb_tls(svc_type) => {
            let port = port.trim().parse().ok()?;
            Some(MdnsService {
                instance: (*instance).to_owned(),
                service_type: svc_type.trim_end_matches('.').to_owned(),
                host: (*host).to_owned(),
                port,
            })
        }
        // Three-column format: instance  type  host:port
        [instance, svc_type, host_port] if is_adb_tls(svc_type) => {
            let (host, port) = split_host_port(host_port)?;
            Some(MdnsService {
                instance: (*instance).to_owned(),
                service_type: svc_type.trim_end_matches('.').to_owned(),
                host,
                port,
            })
        }
        // Three-column legacy format: instance._type  host  port
        [full_name, host, port] => {
            let (instance, svc_type) = split_full_name(full_name)?;
            Some(MdnsService {
                instance,
                service_type: svc_type,
                host: (*host).to_owned(),
                port: port.parse().ok()?,
            })
        }
        // Two-column format: instance._type  host:port
        [full_name, host_port] => {
            let (instance, svc_type) = split_full_name(full_name)?;
            let (host, port) = split_host_port(host_port)?;
            Some(MdnsService {
                instance,
                service_type: svc_type,
                host,
                port,
            })
        }
        _ => None,
    }
}

fn split_host_port(value: &str) -> Option<(String, u16)> {
    if let Some(value) = value.strip_prefix('[') {
        let (host, port) = value.split_once("]:")?;
        return Some((host.to_owned(), port.parse().ok()?));
    }
    let (host, port) = value.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_owned(), port.parse().ok()?))
}

fn is_adb_tls(s: &str) -> bool {
    matches!(s.trim_end_matches('.'), PAIRING_SERVICE | CONNECT_SERVICE)
}

/// Split a full mDNS name like `adb-xyz._adb-tls-pairing._tcp` into
/// `("adb-xyz", "_adb-tls-pairing._tcp")`.
fn split_full_name(full_name: &str) -> Option<(String, String)> {
    const TYPES: &[&str] = &[PAIRING_SERVICE, CONNECT_SERVICE];
    for svc in TYPES {
        if let Some(pos) = full_name.trim_end_matches('.').find(svc) {
            let instance = full_name[..pos].trim_end_matches('.').to_owned();
            if instance.is_empty() {
                return None;
            }
            return Some((instance, (*svc).to_owned()));
        }
    }
    None
}

// ── Worker ────────────────────────────────────────────────────────────────────

/// Spawn a background thread that discovers, pairs, and connects the device.
///
/// Send `cancel.store(true, Ordering::SeqCst)` to abort the worker at any
/// poll point. All progress and results are delivered through `tx`.
///
/// The thread is non-blocking from the caller's perspective; join the returned
/// handle only if you need to wait for clean termination (e.g. in tests).
pub fn start_pairing_worker(
    session: QrSession,
    cancel: Arc<AtomicBool>,
    tx: std::sync::mpsc::Sender<WorkerEvent>,
    verbose: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let runner = CommandAdbRunner::from_path_environment().with_verbose(verbose);
        run_worker(
            &runner,
            &session.service_name,
            &session.password,
            &cancel,
            &tx,
        );
        // `session` is dropped here; `password` falls off the heap.
    })
}

// ── Internal worker logic ─────────────────────────────────────────────────────

const TOTAL_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_PHASE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(1_000);

fn is_cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

/// Query `adb mdns services` and return parsed records.
fn query_mdns(runner: &CommandAdbRunner) -> Result<Vec<MdnsService>, String> {
    let out = runner
        .run(&[OsString::from("mdns"), OsString::from("services")])
        .map_err(|e| e.to_string())?;
    Ok(parse_mdns_services(&String::from_utf8_lossy(&out.stdout)))
}

fn run_worker(
    runner: &CommandAdbRunner,
    service_name: &str,
    password: &str,
    cancel: &AtomicBool,
    tx: &std::sync::mpsc::Sender<WorkerEvent>,
) {
    let _ = tx.send(WorkerEvent::Status(
        "Esperando que el dispositivo escanee el código QR…".to_owned(),
    ));

    let start = Instant::now();

    // ── Phase 1: find _adb-tls-pairing._tcp matching our service name ─────────
    let pairing_svc = loop {
        if is_cancelled(cancel) {
            return;
        }
        if start.elapsed() >= TOTAL_TIMEOUT {
            let _ = tx.send(WorkerEvent::TimedOut);
            return;
        }
        match query_mdns(runner) {
            Err(e) => {
                let _ = tx.send(WorkerEvent::Error(format!("adb mdns: {e}")));
                return;
            }
            Ok(services) => {
                if let Some(svc) = services
                    .into_iter()
                    .find(|s| s.instance == service_name && s.service_type == PAIRING_SERVICE)
                {
                    break svc;
                }
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    // ── Phase 2: pair ─────────────────────────────────────────────────────────
    if is_cancelled(cancel) {
        return;
    }
    let _ = tx.send(WorkerEvent::Status(format!(
        "Emparejando con {}:{}…",
        pairing_svc.host, pairing_svc.port
    )));

    match runner.run(&[
        OsString::from("pair"),
        OsString::from(format_host_port(&pairing_svc.host, pairing_svc.port)),
        OsString::from(password),
    ]) {
        Err(e) => {
            let _ = tx.send(WorkerEvent::Error(format!("adb pair: {e}")));
            return;
        }
        Ok(out) if out.status_code != Some(0) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            let msg = if stderr.is_empty() {
                "El emparejamiento falló (código de salida distinto de cero)".to_owned()
            } else {
                format!("Emparejamiento fallido: {stderr}")
            };
            let _ = tx.send(WorkerEvent::Error(msg));
            return;
        }
        Ok(_) => {}
    }

    // ── Phase 3: find _adb-tls-connect._tcp from the same host ───────────────
    if is_cancelled(cancel) {
        return;
    }
    let _ = tx.send(WorkerEvent::Status(
        "Buscando servicio de conexión…".to_owned(),
    ));

    let paired_host = pairing_svc.host.clone();
    let connect_start = Instant::now();

    let connect_svc = loop {
        if is_cancelled(cancel) {
            return;
        }
        if connect_start.elapsed() >= CONNECT_PHASE_TIMEOUT {
            let _ = tx.send(WorkerEvent::Error(
                "Tiempo agotado buscando el servicio de conexión.".to_owned(),
            ));
            return;
        }
        match query_mdns(runner) {
            Err(e) => {
                let _ = tx.send(WorkerEvent::Error(format!("adb mdns: {e}")));
                return;
            }
            Ok(services) => {
                if let Some(svc) = services
                    .into_iter()
                    .find(|s| s.host == paired_host && s.service_type == CONNECT_SERVICE)
                {
                    break svc;
                }
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    // ── Phase 4: connect ──────────────────────────────────────────────────────
    if is_cancelled(cancel) {
        return;
    }
    let _ = tx.send(WorkerEvent::Status(format!(
        "Conectando a {}:{}…",
        connect_svc.host, connect_svc.port
    )));

    match runner.run(&[
        OsString::from("connect"),
        OsString::from(format_host_port(&connect_svc.host, connect_svc.port)),
    ]) {
        Err(e) => {
            let _ = tx.send(WorkerEvent::Error(format!("adb connect: {e}")));
            return;
        }
        Ok(out) if out.status_code != Some(0) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            let _ = tx.send(WorkerEvent::Error(format!("Conexión fallida: {stderr}")));
            return;
        }
        Ok(_) => {}
    }

    let _ = tx.send(WorkerEvent::Paired);
}

fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── QrSession ──

    #[test]
    fn generated_payload_starts_with_wifi_t_adb_header() {
        let session = QrSession::generate().expect("urandom available");
        assert!(
            session.payload.starts_with("WIFI:T:ADB;S:"),
            "payload must begin with 'WIFI:T:ADB;S:'"
        );
    }

    #[test]
    fn generated_payload_contains_service_name_and_ends_with_double_semicolon() {
        let session = QrSession::generate().expect("urandom available");
        assert!(session.payload.contains(&session.service_name));
        assert!(session.payload.ends_with(";;"));
    }

    #[test]
    fn generated_payload_has_no_whitespace() {
        let session = QrSession::generate().expect("urandom available");
        assert!(!session.payload.chars().any(char::is_whitespace));
    }

    #[test]
    fn service_name_has_correct_prefix_and_length() {
        let session = QrSession::generate().expect("urandom available");
        assert!(session.service_name.starts_with("adb-"));
        // "adb-" (4) + 16 hex chars
        assert_eq!(session.service_name.len(), 20);
    }

    #[test]
    fn service_name_uses_only_safe_characters() {
        let session = QrSession::generate().expect("urandom available");
        for ch in session.service_name.chars() {
            assert!(
                ch.is_ascii_hexdigit() || ch == '-',
                "unexpected char '{ch}' in service_name"
            );
        }
    }

    // ── parse_mdns_services ──

    #[test]
    fn parses_four_column_pairing_record() {
        let output =
            "List of mDNS services:\nadb-deadbeef\t_adb-tls-pairing._tcp\t192.168.1.5\t46159\n";
        let services = parse_mdns_services(output);
        assert_eq!(services.len(), 1);
        let s = &services[0];
        assert_eq!(s.instance, "adb-deadbeef");
        assert_eq!(s.service_type, "_adb-tls-pairing._tcp");
        assert_eq!(s.host, "192.168.1.5");
        assert_eq!(s.port, 46159);
    }

    #[test]
    fn parses_three_column_full_name_record() {
        let output = "adb-deadbeef._adb-tls-pairing._tcp\t192.168.1.5\t46159\n";
        let services = parse_mdns_services(output);
        assert_eq!(services.len(), 1);
        let s = &services[0];
        assert_eq!(s.instance, "adb-deadbeef");
        assert_eq!(s.service_type, "_adb-tls-pairing._tcp");
        assert_eq!(s.host, "192.168.1.5");
        assert_eq!(s.port, 46159);
    }

    #[test]
    fn parses_space_delimited_mdsn_records() {
        let output = "List of discovered mdns services\nadb-deadbeef _adb-tls-pairing._tcp 192.168.1.5 46159\n";
        let services = parse_mdns_services(output);
        assert_eq!(services[0].port, 46159);
    }

    #[test]
    fn parses_current_adb_host_port_records() {
        let output = "List of discovered mdns services\nadb-ZY22G6BQL2-ow7wHl   _adb-tls-connect._tcp   10.0.0.31:39509\n";
        let services = parse_mdns_services(output);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].host, "10.0.0.31");
        assert_eq!(services[0].port, 39509);
    }

    #[test]
    fn formats_ipv6_pairing_addresses() {
        assert_eq!(format_host_port("fe80::1", 46159), "[fe80::1]:46159");
        assert_eq!(format_host_port("192.168.1.5", 46159), "192.168.1.5:46159");
    }

    #[test]
    fn parses_connect_service_record() {
        let output =
            "List of mDNS services:\nadb-guid123\t_adb-tls-connect._tcp\t192.168.1.5\t5555\n";
        let services = parse_mdns_services(output);
        assert_eq!(services.len(), 1);
        assert!(services[0].service_type.contains("_adb-tls-connect"));
    }

    #[test]
    fn skips_header_and_empty_lines() {
        let output = "List of mDNS services:\n\n\nadb-x\t_adb-tls-pairing._tcp\t10.0.0.1\t1234\n";
        assert_eq!(parse_mdns_services(output).len(), 1);
    }

    #[test]
    fn skips_unrelated_service_types() {
        let output = "some-service\t_http._tcp\t10.0.0.1\t80\n";
        assert_eq!(parse_mdns_services(output).len(), 0);
    }

    // ── Correlation ──

    #[test]
    fn pairing_service_matched_by_exact_instance_name() {
        let service_name = "adb-aabbccdd11223344";
        let output = format!(
            "List of mDNS services:\n{service_name}\t_adb-tls-pairing._tcp\t10.0.0.1\t12345\n"
        );
        let services = parse_mdns_services(&output);
        let found = services
            .iter()
            .any(|s| s.instance == service_name && s.service_type.contains("_adb-tls-pairing"));
        assert!(found, "must find pairing service by exact instance name");
    }

    #[test]
    fn connect_service_found_by_host_regardless_of_instance_name() {
        let output =
            "List of mDNS services:\nadb-guid99\t_adb-tls-connect._tcp\t192.168.1.5\t5555\n";
        let services = parse_mdns_services(output);
        let found = services
            .iter()
            .any(|s| s.host == "192.168.1.5" && s.service_type.contains("_adb-tls-connect"));
        assert!(found, "must correlate connect service by host IP");
    }

    // ── split_full_name ──

    #[test]
    fn splits_full_mdns_name_correctly() {
        let (instance, svc) = split_full_name("adb-abc._adb-tls-pairing._tcp").expect("parseable");
        assert_eq!(instance, "adb-abc");
        assert_eq!(svc, "_adb-tls-pairing._tcp");
    }

    #[test]
    fn returns_none_for_unknown_service_type() {
        assert!(split_full_name("my-service._http._tcp").is_none());
    }
}
