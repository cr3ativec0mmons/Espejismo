# Espejismo

Espejismo es una interfaz experimental de escritorio para ADB y scrcpy, escrita en Rust con GTK4 y libadwaita. El núcleo contiene el modelo de dispositivos, estado de aplicación, persistencia JSON versionada, cliente ADB seguro y launcher de scrcpy.

La primera superficie GTK ya incluye una ventana principal siguiendo la jerarquía de libadwaita, lista de dispositivos ADB, estados de diagnóstico, acción de actualización, flujo de emparejamiento Wi-Fi y lanzamiento de scrcpy mediante `Observar`. La opción `--verbose` muestra en la terminal las operaciones ADB, ocultando el código de pairing.

## Desarrollo

Requisitos del núcleo:

- Rust estable y Cargo.

Para activar la interfaz gráfica también se necesitan los paquetes de desarrollo `gtk4`, `libadwaita`, GLib, GObject, Pango, Cairo y GDK-Pixbuf de la distribución. En Fedora suelen corresponder a `gtk4-devel`, `libadwaita-devel`, `glib2-devel`, `pango-devel`, `cairo-devel` y `gdk-pixbuf2-devel`.

Comandos de verificación:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

La feature `gui` habilita el binario GTK:

```text
cargo run --features gui
```

Para probar la interfaz experimental se necesitan además una sesión gráfica Wayland o X11. El diálogo de emparejamiento no persiste el código introducido.

ADB, Platform-Tools, scrcpy y las reglas udev para Android son dependencias externas del paquete nativo. La aplicación no persistirá códigos de pairing ni ejecutará comandos mediante un shell.

El núcleo puede validarse sin bibliotecas GTK:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

El build `cargo check --features gui` requiere los paquetes de desarrollo anteriores y una sesión Wayland o X11 para ejecutar la aplicación. La integración Flatpak se añadirá después de fijar versiones y checksums verificables de Platform-Tools y scrcpy.