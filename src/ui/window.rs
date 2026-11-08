use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
#[allow(deprecated)]
use gtk4::{Align, Box, Button, ComboBoxText, Image, Label, ListBox, ListBoxRow, Orientation};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::adb::{AdbClient, CommandAdbRunner, Endpoint};
use crate::app::AppController;
use crate::config::ConfigRepository;
use crate::model::{Decoder, Device, DeviceStatus, Transport};
use crate::scrcpy::ScrcpyLauncher;

type Controller = Rc<RefCell<AppController<CommandAdbRunner>>>;

/// Crea y presenta la ventana principal de la aplicación.
pub fn present(application: &adw::Application, verbose: bool) {
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Espejismo")
        .default_width(760)
        .default_height(560)
        .build();
    let controller = match create_controller(verbose) {
        Ok(controller) => Rc::new(RefCell::new(controller)),
        Err(error) => {
            let page = adw::StatusPage::builder()
                .title("No se puede cargar Espejismo")
                .description(error.to_string())
                .build();
            window.set_content(Some(&page));
            window.present();
            return;
        }
    };
    let controller_for_shutdown = controller.clone();
    application.connect_shutdown(move |_| {
        if let Err(error) = controller_for_shutdown.borrow().shutdown() {
            tracing::warn!(%error, "could not stop ADB server during shutdown");
        }
    });
    let content = build_content(&window, controller, verbose);
    window.set_content(Some(&content));
    window.present();
}

/// Construye la barra de herramientas y la vista principal de dispositivos.
fn build_content(window: &adw::ApplicationWindow, controller: Controller, verbose: bool) -> Box {
    let content = Box::new(Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();

    let refresh = Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Actualizar dispositivos"));
    let add = Button::from_icon_name("list-add-symbolic");
    add.set_tooltip_text(Some("Agregar dispositivo"));
    let settings = Button::from_icon_name("emblem-system-symbolic");
    settings.set_tooltip_text(Some("Configuración de scrcpy"));
    header.pack_start(&refresh);
    header.pack_end(&settings);
    header.pack_end(&add);
    content.append(&header);

    let toast_overlay = adw::ToastOverlay::new();
    let page = adw::StatusPage::builder()
        .title("Tus dispositivos")
        .description("Conecta un dispositivo Android por USB o agrégalo por Wi-Fi.")
        .icon_name("phone-symbolic")
        .build();
    let list = ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("boxed-list");
    list.set_margin_start(24);
    list.set_margin_end(24);
    list.set_margin_top(12);
    list.set_margin_bottom(24);
    page.set_child(Some(&list));
    toast_overlay.set_child(Some(&page));
    content.append(&toast_overlay);

    refresh_devices(&list, &controller, &toast_overlay);

    let list_for_refresh = list.clone();
    let controller_for_refresh = controller.clone();
    let toast_overlay_for_refresh = toast_overlay.clone();
    refresh.connect_clicked(move |_| {
        refresh_devices(
            &list_for_refresh,
            &controller_for_refresh,
            &toast_overlay_for_refresh,
        );
    });
    let window_for_pairing = window.clone();
    let list_for_pairing = list.clone();
    let controller_for_pairing = controller.clone();
    let toast_overlay_for_pairing = toast_overlay.clone();
    add.connect_clicked(move |_| {
        let list = list_for_pairing.clone();
        let controller = controller_for_pairing.clone();
        let toast_overlay = toast_overlay_for_pairing.clone();
        let list_for_qr = list.clone();
        let controller_for_qr = controller.clone();
        let toast_overlay_for_qr = toast_overlay.clone();
        super::pairing_dialog::present(
            &window_for_pairing,
            move |details| {
                let host = details.host;
                let result = Endpoint::new(host.clone(), details.pairing_port)
                    .and_then(|pairing_endpoint| {
                        Endpoint::new(host, details.adb_port)
                            .map(|adb_endpoint| (pairing_endpoint, adb_endpoint))
                    })
                    .map_err(crate::app::AppError::from)
                    .and_then(|(pairing_endpoint, adb_endpoint)| {
                        controller.borrow_mut().pair_and_connect(
                            &pairing_endpoint,
                            &details.code,
                            &adb_endpoint,
                        )
                    });
                match result {
                    Ok(_) => populate_list(&list, &controller, &toast_overlay),
                    Err(error) => toast_overlay.add_toast(adw::Toast::new(&error.to_string())),
                }
            },
            move || populate_list(&list_for_qr, &controller_for_qr, &toast_overlay_for_qr),
            verbose,
        );
    });

    let window_for_settings = window.clone();
    let controller_for_settings = controller.clone();
    let toast_for_settings = toast_overlay.clone();
    settings.connect_clicked(move |_| {
        present_settings(
            &window_for_settings,
            &controller_for_settings,
            &toast_for_settings,
        );
    });

    content
}

#[allow(deprecated)]
fn present_settings(
    parent: &adw::ApplicationWindow,
    controller: &Controller,
    toast_overlay: &adw::ToastOverlay,
) {
    let current = controller.borrow().settings().scrcpy.clone();
    let dialog = adw::Dialog::builder()
        .title("Configuración de scrcpy")
        .content_width(360)
        .build();
    let content = Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(18)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let codec = ComboBoxText::new();
    codec.append(Some("h264"), "H.264");
    codec.append(Some("hevc"), "HEVC");
    codec.append(Some("vp9"), "VP9");
    codec.append(Some("av1"), "AV1");
    let current_codec = match current.codec.as_deref() {
        Some("h265") => "hevc",
        Some(value) => value,
        None => "h264",
    };
    codec.set_active_id(Some(current_codec));

    let decoder = ComboBoxText::new();
    decoder.append(Some("auto"), "Automático");
    decoder.append(Some("vaapi"), "VAAPI");
    decoder.append(Some("software"), "Software");
    decoder.set_active_id(Some(match current.decoder {
        Decoder::Auto => "auto",
        Decoder::Vaapi => "vaapi",
        Decoder::Software => "software",
    }));

    let codec_row = setting_row("Códec de vídeo", &codec);
    let decoder_row = setting_row("Decodificación", &decoder);
    content.append(&codec_row);
    content.append(&decoder_row);

    let vaapi_hint = Label::builder()
        .label("VAAPI requiere un controlador compatible; Automático usa software como respaldo.")
        .css_classes(["dim-label", "caption"])
        .wrap(true)
        .xalign(0.0)
        .build();
    content.append(&vaapi_hint);

    let save = Button::builder()
        .label("Guardar")
        .halign(Align::End)
        .css_classes(["suggested-action"])
        .build();
    content.append(&save);
    dialog.set_child(Some(&content));

    let controller = controller.clone();
    let toast_overlay = toast_overlay.clone();
    let dialog_for_save = dialog.clone();
    save.connect_clicked(move |_| {
        let selected_codec = codec.active_id().map(|value| value.to_string());
        let selected_decoder = match decoder.active_id().as_deref() {
            Some("vaapi") => Decoder::Vaapi,
            Some("software") => Decoder::Software,
            _ => Decoder::Auto,
        };
        match controller
            .borrow_mut()
            .update_scrcpy_settings(selected_codec, selected_decoder)
        {
            Ok(()) => dialog_for_save.force_close(),
            Err(error) => toast_overlay.add_toast(adw::Toast::new(&error.to_string())),
        }
    });
    dialog.present(Some(parent));
}

fn setting_row<W: IsA<gtk4::Widget>>(title: &str, widget: &W) -> Box {
    let row = Box::new(Orientation::Horizontal, 12);
    let label = Label::builder()
        .label(title)
        .hexpand(true)
        .xalign(0.0)
        .build();
    row.append(&label);
    row.append(widget);
    row
}

/// Construye los servicios de la aplicación para la ventana principal.
fn create_controller(
    verbose: bool,
) -> Result<AppController<CommandAdbRunner>, crate::app::AppError> {
    let repository = ConfigRepository::discover()?;
    let adb = AdbClient::new(CommandAdbRunner::from_path_environment().with_verbose(verbose));
    let scrcpy = ScrcpyLauncher::from_path_environment();
    AppController::new(repository, adb, scrcpy)
}

/// Actualiza ADB y muestra los dispositivos observados.
fn refresh_devices(list: &ListBox, controller: &Controller, toast_overlay: &adw::ToastOverlay) {
    if let Err(error) = controller.borrow_mut().refresh() {
        clear_list(list);
        toast_overlay.add_toast(adw::Toast::new(&error.to_string()));
        return;
    }
    populate_list(list, controller, toast_overlay);
}

/// Muestra el estado actual del controlador sin volver a ejecutar ADB.
fn populate_list(list: &ListBox, controller: &Controller, toast_overlay: &adw::ToastOverlay) {
    clear_list(list);
    let devices = controller.borrow().devices().to_vec();
    for device in devices {
        list.append(&device_row(&device, controller, list, toast_overlay));
    }
}

/// Elimina las filas actuales antes de pintar el resultado de una operación.
fn clear_list(list: &ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

/// Construye una fila de lista con la información y acciones del dispositivo.
fn device_row(
    device: &Device,
    controller: &Controller,
    list: &ListBox,
    toast_overlay: &adw::ToastOverlay,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    let content = Box::new(Orientation::Horizontal, 12);
    content.set_margin_top(14);
    content.set_margin_bottom(14);
    content.set_margin_start(16);
    content.set_margin_end(16);

    let (icon_name, transport_label) = match device.transport {
        Transport::Usb => ("drive-removable-media-symbolic", "Conexión USB"),
        Transport::Wifi => ("network-wireless-symbolic", "Conexión inalámbrica"),
    };
    let icon = Image::from_icon_name(icon_name);
    icon.set_tooltip_text(Some(transport_label));
    icon.update_property(&[gtk4::accessible::Property::Label(transport_label)]);

    let details = Box::new(Orientation::Vertical, 3);
    details.set_hexpand(true);
    details.append(
        &Label::builder()
            .label(device.display_name())
            .xalign(0.0)
            .build(),
    );
    let serial = Label::builder()
        .label(device.id.as_str())
        .css_classes(["dim-label", "caption"])
        .xalign(0.0)
        .build();
    details.append(&serial);

    let status = status_label(device.status);
    let action = if device.is_usable() {
        let button = Button::with_label("Observar");
        button.add_css_class("suggested-action");
        let controller = controller.clone();
        let device_id = device.id.clone();
        let toast_overlay = toast_overlay.clone();
        button.connect_clicked(move |_| {
            if let Err(error) = controller.borrow_mut().launch_scrcpy(&device_id) {
                toast_overlay.add_toast(adw::Toast::new(&error.to_string()));
            }
        });
        button
    } else {
        let button = Button::with_label("Diagnóstico");
        button.add_css_class("flat");
        let toast_overlay = toast_overlay.clone();
        let message = diagnostic_message(device.status).to_owned();
        button.connect_clicked(move |_| {
            toast_overlay.add_toast(adw::Toast::new(&message));
        });
        button
    };
    action.set_valign(Align::Center);

    content.append(&icon);
    content.append(&details);
    content.append(&status);
    content.append(&action);
    let remove = Button::from_icon_name("user-trash-symbolic");
    remove.set_tooltip_text(Some("Quitar dispositivo"));
    remove.add_css_class("flat");
    let device_id = device.id.clone();
    let controller = controller.clone();
    let list = list.clone();
    let toast_overlay = toast_overlay.clone();
    remove.connect_clicked(
        move |_| match controller.borrow_mut().remove_device(&device_id) {
            Ok(()) => populate_list(&list, &controller, &toast_overlay),
            Err(error) => toast_overlay.add_toast(adw::Toast::new(&error.to_string())),
        },
    );
    remove.set_valign(Align::Center);
    content.append(&remove);
    row.set_child(Some(&content));
    row
}

/// Crea la etiqueta visual correspondiente al estado ADB indicado.
fn status_label(status: DeviceStatus) -> Label {
    let (text, css_class) = match status {
        DeviceStatus::Device => ("Listo", "success"),
        DeviceStatus::Offline => ("Desconectado", "warning"),
        DeviceStatus::Unauthorized => ("No autorizado", "warning"),
        DeviceStatus::NoPermissions => ("Sin permisos", "error"),
        DeviceStatus::Unknown => ("Estado desconocido", "dim-label"),
    };
    Label::builder()
        .label(text)
        .css_classes(["caption", css_class])
        .valign(Align::Center)
        .build()
}

/// Devuelve un mensaje de diagnóstico para el estado del dispositivo.
fn diagnostic_message(status: DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Offline => "El dispositivo está desconectado. Comprueba el cable o la red.",
        DeviceStatus::Unauthorized => "Desbloquea el dispositivo y acepta la autorización USB.",
        DeviceStatus::NoPermissions => "Añade una regla udev para conceder acceso al dispositivo.",
        DeviceStatus::Unknown => "ADB devolvió un estado no reconocido.",
        DeviceStatus::Device => "El dispositivo está listo.",
    }
}
