use gtk4::prelude::*;
use gtk4::{Align, Dialog, Entry, Grid, Label, SpinButton, Window};

pub struct PairingDetails {
    pub host: String,
    pub pairing_port: u16,
    pub code: String,
    pub adb_port: u16,
}

/// Crea y presenta el diálogo para emparejar un dispositivo por Wi-Fi.
pub fn present(parent: &impl IsA<Window>, on_connect: impl Fn(PairingDetails) + 'static) {
    let dialog = Dialog::builder()
        .modal(true)
        .transient_for(parent)
        .title("Agregar dispositivo")
        .build();
    dialog.add_button("Cancelar", gtk4::ResponseType::Cancel);
    dialog.add_button("Conectar", gtk4::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(18);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let explanation = Label::builder()
        .label("Introduce los datos mostrados por Android para emparejarlo por Wi-Fi.")
        .wrap(true)
        .xalign(0.0)
        .build();
    content.append(&explanation);

    let fields = Grid::builder()
        .column_spacing(12)
        .row_spacing(12)
        .halign(Align::Fill)
        .build();
    let host = Entry::builder()
        .placeholder_text("192.168.1.20")
        .hexpand(true)
        .build();
    let pairing_port = SpinButton::with_range(1.0, 65535.0, 1.0);
    pairing_port.set_value(37099.0);
    let adb_port = SpinButton::with_range(1.0, 65535.0, 1.0);
    adb_port.set_value(5555.0);
    let code = Entry::builder()
        .visibility(false)
        .input_purpose(gtk4::InputPurpose::Digits)
        .hexpand(true)
        .build();

    add_field(&fields, "Dirección IP", &host, 0);
    add_field(&fields, "Puerto de emparejamiento", &pairing_port, 1);
    add_field(&fields, "Código de emparejamiento", &code, 2);
    add_field(&fields, "Puerto ADB", &adb_port, 3);
    content.append(&fields);

    let note = Label::builder()
        .label("El código se usará solo durante esta conexión y no se guardará.")
        .css_classes(["dim-label"])
        .wrap(true)
        .xalign(0.0)
        .build();
    content.append(&note);

    dialog.connect_response(move |dialog, response| {
        if response != gtk4::ResponseType::Accept {
            dialog.close();
            return;
        }
        if host.text().trim().is_empty() || code.text().trim().is_empty() {
            return;
        }
        on_connect(PairingDetails {
            host: host.text().trim().to_owned(),
            pairing_port: pairing_port.value_as_int() as u16,
            code: code.text().trim().to_owned(),
            adb_port: adb_port.value_as_int() as u16,
        });
        dialog.close();
    });
    dialog.present();
}

/// Añade una etiqueta y su control asociado a una fila del formulario.
fn add_field<T: IsA<gtk4::Widget>>(grid: &Grid, title: &str, field: &T, row: i32) {
    let label = Label::builder()
        .label(title)
        .halign(Align::Start)
        .valign(Align::Center)
        .build();
    grid.attach(&label, 0, row, 1, 1);
    grid.attach(field, 1, row, 1, 1);
}
