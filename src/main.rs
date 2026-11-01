#[cfg(feature = "gui")]
fn main() {
    use gio::prelude::*;
    use gtk4::prelude::*;
    use libadwaita as adw;

    tracing_subscriber::fmt::init();
    let application = adw::Application::builder()
        .application_id(espejismo::APPLICATION_ID)
        .build();

    application.connect_activate(|application| {
        let window = adw::ApplicationWindow::builder()
            .application(application)
            .title("Espejismo")
            .default_width(720)
            .default_height(480)
            .build();
        window.present();
    });

    application.run();
}

#[cfg(not(feature = "gui"))]
fn main() {
    tracing_subscriber::fmt::init();
    tracing::info!("Espejismo core bootstrap; compile with --features gui for the GTK application");
}
