#[cfg(feature = "gui")]
/// Inicializa y ejecuta la aplicación GTK cuando se compila la interfaz gráfica.
fn main() {
    use libadwaita as adw;
    use libadwaita::gio::prelude::{ApplicationExt, ApplicationExtManual};

    let arguments = std::env::args()
        .filter(|argument| argument != "--verbose" && argument != "-v")
        .collect::<Vec<_>>();
    let verbose = std::env::args().any(|argument| argument == "--verbose" || argument == "-v");
    tracing_subscriber::fmt()
        .with_max_level(if verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::WARN
        })
        .init();
    let application = adw::Application::builder()
        .application_id(espejismo::APPLICATION_ID)
        .build();

    application.connect_activate(move |application| {
        espejismo::ui::window::present(application, verbose);
    });

    application.run_with_args(&arguments);
}

#[cfg(not(feature = "gui"))]
/// Inicializa el binario principal en la compilación sin interfaz gráfica.
fn main() {
    let verbose = std::env::args().any(|argument| argument == "--verbose" || argument == "-v");
    tracing_subscriber::fmt()
        .with_max_level(if verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::WARN
        })
        .init();
    tracing::info!("Espejismo core bootstrap; compile with --features gui for the GTK application");
}
