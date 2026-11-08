//! Dialog for pairing an Android device over Wi-Fi.
//!
//! The dialog has two pages selected with [`adw::InlineViewSwitcher`]:
//!
//! - **Manual** – the user enters the host, ports, and pairing code shown by
//!   Android's Developer Options.  Calls `on_connect(PairingDetails)` on
//!   success.
//!
//! - **QR** – Espejismo generates a random service name + password, renders
//!   them as a QR code, and launches a background worker that discovers the
//!   device via mDNS, pairs it, and connects.  Calls `on_paired()` on
//!   success.  The worker is cancelled when the dialog closes.
//!
//! # API contract
//!
//! ```ignore
//! pairing_dialog::present(
//!     &window,
//!     |details| { /* manual pairing: use details.host/pairing_port/code/adb_port */ },
//!     || { /* QR pairing succeeded: refresh device list */ },
//! );
//! ```
//!
//! `on_paired` is called on the GTK main thread and may freely access
//! GTK objects.  It is called at most once per dialog invocation.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gtk4::cairo;
use gtk4::prelude::*;
use gtk4::{Align, Box, DrawingArea, Entry, Grid, Label, Orientation, SpinButton};
use libadwaita as adw;
use libadwaita::prelude::*;
use qrcode::QrCode;

use crate::adb::qr::{QrSession, WorkerEvent, start_pairing_worker};

// ── Public types ──────────────────────────────────────────────────────────────

/// Details collected from the Manual page, forwarded to `on_connect`.
pub struct PairingDetails {
    pub host: String,
    pub pairing_port: u16,
    pub code: String,
    pub adb_port: u16,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Create and present the pairing dialog.
///
/// # Parameters
///
/// - `parent` – transient parent widget (typically the application window).
/// - `on_connect` – called when the user submits the Manual form.  Receives
///   validated [`PairingDetails`].  Called on the GTK main thread.
/// - `on_paired` – called once when QR pairing completes successfully.
///   Called on the GTK main thread.  The dialog is closed before the call.
pub fn present(
    parent: &impl IsA<gtk4::Widget>,
    on_connect: impl Fn(PairingDetails) + 'static,
    on_paired: impl Fn() + 'static,
) {
    // ── QR session (generated once per dialog) ────────────────────────────────
    let (qr_width, qr_colors, session) = match build_qr_data() {
        Some(v) => v,
        None => {
            show_error_dialog(parent, "No se pudo generar el código QR.");
            return;
        }
    };

    // ── Dialog ────────────────────────────────────────────────────────────────
    let dialog = adw::Dialog::builder()
        .title("Agregar dispositivo")
        .content_width(440)
        .build();

    // ── View switcher + stack ─────────────────────────────────────────────────
    let stack = adw::ViewStack::new();
    let switcher = adw::InlineViewSwitcher::new();
    switcher.set_stack(Some(&stack));

    // ── Header ────────────────────────────────────────────────────────────────
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&switcher));

    // ── Manual page ───────────────────────────────────────────────────────────
    let manual_page = build_manual_page();
    stack.add_titled(&manual_page.root, Some("manual"), "Manual");

    // ── QR page ───────────────────────────────────────────────────────────────
    let (qr_root, status_label) = build_qr_page(qr_width, qr_colors);
    stack.add_titled(&qr_root, Some("qr"), "Código QR");

    // ── Layout ────────────────────────────────────────────────────────────────
    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&stack));
    dialog.set_child(Some(&toolbar_view));

    // ── Worker state (Rc because it lives on the main thread only) ────────────
    let cancel: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let timer_active: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // ── Cancel worker on dialog close ─────────────────────────────────────────
    {
        let cancel2 = Arc::clone(&cancel);
        let timer_active2 = Rc::clone(&timer_active);
        dialog.connect_closed(move |_| {
            cancel2.store(true, Ordering::SeqCst);
            timer_active2.set(false);
        });
    }

    // ── Start worker + poll timer when QR page is shown ───────────────────────
    start_qr_worker(
        session,
        Arc::clone(&cancel),
        Rc::clone(&timer_active),
        status_label,
        dialog.clone(),
        on_paired,
    );
    timer_active.set(true);

    // ── Manual page "Conectar" button ─────────────────────────────────────────
    {
        let dialog2 = dialog.clone();
        manual_page.connect_btn.connect_clicked(move |_| {
            let host = manual_page.host.text().trim().to_owned();
            if host.is_empty() {
                return;
            }
            let code = manual_page.code.text().trim().to_owned();
            if code.is_empty() {
                return;
            }
            on_connect(PairingDetails {
                host,
                pairing_port: manual_page.pairing_port.value_as_int() as u16,
                code,
                adb_port: manual_page.adb_port.value_as_int() as u16,
            });
            dialog2.force_close();
        });
    }

    // ── Present ───────────────────────────────────────────────────────────────
    dialog.present(Some(parent));
}

// ── QR data preparation ───────────────────────────────────────────────────────

/// Pre-compute everything derived from a fresh [`QrSession`].
///
/// Returns `(qr_width, qr_colors_row_major, session)` or `None`
/// if session generation or QR encoding fails.
#[allow(clippy::type_complexity)]
fn build_qr_data() -> Option<(usize, Vec<qrcode::Color>, QrSession)> {
    let session = QrSession::generate().ok()?;
    let qr = QrCode::new(session.payload.as_bytes()).ok()?;
    let width = qr.width();
    // Collect the colour matrix in row-major order: index = row * width + col.
    let colors: Vec<qrcode::Color> = (0..width)
        .flat_map(|row| (0..width).map(move |col| (row, col)))
        .map(|(row, col)| qr[(col, row)])
        .collect();
    Some((width, colors, session))
}

// ── Worker launch ─────────────────────────────────────────────────────────────

fn start_qr_worker(
    session: QrSession,
    cancel: Arc<AtomicBool>,
    timer_active: Rc<Cell<bool>>,
    status_label: Label,
    dialog: adw::Dialog,
    on_paired: impl Fn() + 'static,
) {
    let (tx, rx) = mpsc::channel::<WorkerEvent>();

    start_pairing_worker(session, cancel, tx);

    let on_paired = Rc::new(on_paired);
    let timer_active2 = Rc::clone(&timer_active);

    gtk4::glib::timeout_add_local(Duration::from_millis(300), move || {
        if !timer_active2.get() {
            return gtk4::glib::ControlFlow::Break;
        }
        match rx.try_recv() {
            Ok(WorkerEvent::Status(msg)) => {
                status_label.set_label(&msg);
                gtk4::glib::ControlFlow::Continue
            }
            Ok(WorkerEvent::Paired) => {
                timer_active2.set(false);
                // Close dialog first so on_paired sees a fresh device list.
                dialog.force_close();
                on_paired();
                gtk4::glib::ControlFlow::Break
            }
            Ok(WorkerEvent::TimedOut) => {
                timer_active2.set(false);
                status_label.set_label("Tiempo agotado. Vuelve a intentarlo.");
                gtk4::glib::ControlFlow::Break
            }
            Ok(WorkerEvent::Error(msg)) => {
                timer_active2.set(false);
                status_label.set_label(&format!("Error: {msg}"));
                gtk4::glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => gtk4::glib::ControlFlow::Break,
        }
    });
}

// ── Manual page ───────────────────────────────────────────────────────────────

struct ManualPage {
    root: Box,
    host: Entry,
    pairing_port: SpinButton,
    code: Entry,
    adb_port: SpinButton,
    connect_btn: gtk4::Button,
}

fn build_manual_page() -> ManualPage {
    let vbox = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let explanation = Label::builder()
        .label("Introduce los datos mostrados por Android para emparejarlo por Wi-Fi.")
        .wrap(true)
        .xalign(0.0)
        .build();
    vbox.append(&explanation);

    let fields = Grid::builder()
        .column_spacing(12)
        .row_spacing(12)
        .halign(Align::Fill)
        .build();

    let host = Entry::builder()
        .placeholder_text("192.168.1.20")
        .hexpand(true)
        .build();
    let pairing_port = SpinButton::with_range(1.0, 65_535.0, 1.0);
    pairing_port.set_value(37_099.0);
    let adb_port = SpinButton::with_range(1.0, 65_535.0, 1.0);
    adb_port.set_value(5_555.0);
    let code = Entry::builder()
        .visibility(false)
        .input_purpose(gtk4::InputPurpose::Digits)
        .hexpand(true)
        .build();

    attach_field(&fields, "Dirección IP", &host, 0);
    attach_field(&fields, "Puerto de emparejamiento", &pairing_port, 1);
    attach_field(&fields, "Código de emparejamiento", &code, 2);
    attach_field(&fields, "Puerto ADB", &adb_port, 3);
    vbox.append(&fields);

    let note = Label::builder()
        .label("El código se usará solo durante esta conexión y no se guardará.")
        .css_classes(["dim-label"])
        .wrap(true)
        .xalign(0.0)
        .build();
    vbox.append(&note);

    let connect_btn = gtk4::Button::builder()
        .label("Conectar")
        .css_classes(["suggested-action"])
        .halign(Align::End)
        .build();
    vbox.append(&connect_btn);

    ManualPage {
        root: vbox,
        host,
        pairing_port,
        code,
        adb_port,
        connect_btn,
    }
}

fn attach_field<W: IsA<gtk4::Widget>>(grid: &Grid, title: &str, widget: &W, row: i32) {
    let label = Label::builder()
        .label(title)
        .halign(Align::Start)
        .valign(Align::Center)
        .build();
    grid.attach(&label, 0, row, 1, 1);
    grid.attach(widget, 1, row, 1, 1);
}

// ── QR page ───────────────────────────────────────────────────────────────────

fn build_qr_page(qr_width: usize, qr_colors: Vec<qrcode::Color>) -> (Box, Label) {
    let vbox = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .halign(Align::Center)
        .build();

    let canvas = DrawingArea::builder()
        .width_request(240)
        .height_request(240)
        .build();

    canvas.set_draw_func(move |_da, cr, w, h| {
        draw_qr_code(cr, w, h, &qr_colors, qr_width);
    });

    let instruction = Label::builder()
        .label("Escanea este código QR desde\nAjustes → Opciones para desarrolladores → Depuración inalámbrica → Emparejar con código QR")
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .xalign(0.5)
        .build();

    let status = Label::builder()
        .label("Esperando que el dispositivo escanee el código QR…")
        .css_classes(["dim-label"])
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .xalign(0.5)
        .build();

    vbox.append(&canvas);
    vbox.append(&instruction);
    vbox.append(&status);

    (vbox, status)
}

// ── QR renderer ──────────────────────────────────────────────────────────────

/// Draw the QR code matrix onto a Cairo context.
///
/// `colors` must be in row-major order (produced by `build_qr_data`).
fn draw_qr_code(cr: &cairo::Context, w: i32, h: i32, colors: &[qrcode::Color], qr_width: usize) {
    const QUIET_MODULES: usize = 4; // ISO 18004 mandates ≥ 4 module quiet zone

    let total_modules = (qr_width + 2 * QUIET_MODULES) as f64;
    let size = (w as f64).min(h as f64);
    let scale = size / total_modules;

    // White background (also covers the quiet zone)
    cr.set_source_rgb(1.0, 1.0, 1.0);
    let _ = cr.paint();

    // Dark (black) modules
    cr.set_source_rgb(0.0, 0.0, 0.0);
    for row in 0..qr_width {
        for col in 0..qr_width {
            if colors[row * qr_width + col] == qrcode::Color::Dark {
                let x = (col + QUIET_MODULES) as f64 * scale;
                let y = (row + QUIET_MODULES) as f64 * scale;
                cr.rectangle(x, y, scale, scale);
            }
        }
    }
    let _ = cr.fill();
}

// ── Fallback error dialog ─────────────────────────────────────────────────────

fn show_error_dialog(parent: &impl IsA<gtk4::Widget>, message: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading("Error")
        .body(message)
        .build();
    dialog.add_response("ok", "Aceptar");
    dialog.present(Some(parent));
}
