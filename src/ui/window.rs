use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box, Button, Label, ListBox, ListBoxRow, Orientation};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::adb::{AdbClient, CommandAdbRunner, Endpoint};
use crate::app::AppController;
use crate::config::ConfigRepository;
use crate::model::{Device, DeviceStatus, Transport};
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
    let content = build_content(&window, controller);
    window.set_content(Some(&content));
    window.present();
}

/// Construye la barra de herramientas y la vista principal de dispositivos.
fn build_content(window: &adw::ApplicationWindow, controller: Controller) -> Box {
    let content = Box::new(Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();

    let refresh = Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Actualizar dispositivos"));
    let add = Button::from_icon_name("list-add-symbolic");
    add.set_tooltip_text(Some("Agregar dispositivo"));
    header.pack_start(&refresh);
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
        super::pairing_dialog::present(&window_for_pairing, move |details| {
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
        });
    });

    content
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

    let icon = Label::new(Some(if device.transport == Transport::Usb {
        "USB"
    } else {
        "Wi-Fi"
    }));
    icon.add_css_class("caption-heading");
    icon.set_width_chars(6);
    icon.set_xalign(0.0);

    let details = Box::new(Orientation::Vertical, 3);
    details.set_hexpand(true);
    let title = device
        .model
        .as_deref()
        .or(device.alias.as_deref())
        .unwrap_or("Dispositivo Android");
    details.append(&Label::builder().label(title).xalign(0.0).build());
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
