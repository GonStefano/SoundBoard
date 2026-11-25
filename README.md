# Soundboard

Soundboard es una aplicación de escritorio escrita en Rust para reproducir efectos de audio por los altavoces y enviarlos simultáneamente a un dispositivo virtual que otras aplicaciones puedan usar como microfono.

Los archivos admitidos son MP3, WAV, OGG y FLAC. La aplicacion permite asignar atajos globales, ajustar el volumen general e individual y mantener una configuracion persistente.

## Requisitos

### Windows

Descargar e instalar:

- [VB-CABLE](https://vb-audio.com/Cable/) como dispositivo de microfono virtual.
- La version de Soundboard preparada para Windows, si existe una release publicada.

El ejecutable no requiere tener Rust instalado. Para compilarlo desde el codigo fuente se necesita Rust y el toolchain de Windows.

### Linux

Se necesita PipeWire o PulseAudio. Para una configuracion sencilla se puede crear temporalmente un sink virtual con PipeWire/PulseAudio:

```bash
pactl load-module module-null-sink sink_name=Soundboard sink_properties=device.description=Soundboard
```

El ejecutable debe ejecutarse en el entorno grafico del usuario.

### Compilar desde el codigo fuente

Instalar Rust desde [rustup](https://rustup.rs/) y ejecutar:

```bash
git clone <URL-DEL-REPOSITORIO>
cd Soundboard
cargo run --release
```

En Windows, la compilacion multiplataforma desde Linux puede requerir el target MinGW:

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

## Configuracion con cable virtual

### Windows

1. Instalar VB-CABLE y reiniciar el equipo si el instalador lo solicita.
2. Abrir Soundboard.
3. En **Auriculares** seleccionar los altavoces o auriculares desde los que se desea escuchar.
4. En **Cable virtual** seleccionar `CABLE Input`.
5. Hacer clic en **Actualizar dispositivos** si el cable no aparece.
6. En Discord, OBS, juegos u otra aplicacion, seleccionar `CABLE Output` como microfono.
7. Asignar un atajo o reproducir cada sonido desde la tarjeta correspondiente.

`CABLE Input` recibe el audio desde Soundboard. `CABLE Output` es el dispositivo que otras aplicaciones usan como entrada de microfono.

### Linux

1. Ejecutar el comando anterior para crear el sink virtual.
2. Abrir Soundboard.
3. Seleccionar `Soundboard` como **Cable virtual**.
4. Seleccionar los auriculares como **Auriculares** si se desea escuchar los efectos.
5. En Discord, OBS, juegos u otra aplicacion, seleccionar `Monitor of Soundboard` o `Soundboard.monitor` como microfono.
6. Hacer clic en **Actualizar dispositivos** si los dispositivos no aparecen.

Para mantener el sink virtual al reiniciar el equipo, configura `module-null-sink` en PipeWire/WirePlumber segun su configuracion local.

## Uso

- **Agregar sonido:** abre un explorador para elegir un archivo MP3, WAV, OGG o FLAC.
- **Editar:** cambia el nombre, el volumen o el atajo global del sonido.
- **Borrar:** elimina el sonido de la configuracion.
- **Detener:** interrumpe todas las reproducciones.
- **Actualizar dispositivos:** vuelve a buscar altavoces, auriculares y cables virtuales.

La configuracion se guarda automaticamente al cerrar la aplicacion.

## Atajos

Los atajos se capturan dentro de la interfaz. Pueden usar teclas fisicas como corchetes, modificadores u otras teclas compatibles con global-hotkey.

## Compilacion y pruebas

```bash
cargo fmt --all -- --check
cargo check --message-format=short
cargo test --bin soundboard
cargo build --release
```

## Documentacion

- [Roadmap del proyecto](docs/ROADMAP.md)
