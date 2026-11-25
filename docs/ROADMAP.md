# Soundboard en Rust — Roadmap

## Estado Actual

- Preparación inicial para GitHub completada: repositorio Git inicializado y `.gitignore` ampliado para excluir compilados, binarios, archivos locales de IDE y archivos temporales.
- README creado con descripción, requisitos, descarga de VB-CABLE, configuración de cable virtual en Windows y Linux, compilación y uso.
- Fase 0 completada: roadmap, contratos iniciales y checklist creados.
- Fase 1 estructural completada: manifiesto Cargo y esqueleto de módulos contractuales creados.
- Fase 3 completada: subsistemas ensamblados; `cargo check` y `cargo build --release` terminan sin warnings.
- Binario Linux generado: `target/release/soundboard` (13 MiB).
- Binario Windows GNU generado: `target/x86_64-pc-windows-gnu/release/soundboard.exe` (7.3 MiB; PE32+ x86-64).
- Binario Windows recompilado como GUI sin ventana de consola: `target/x86_64-pc-windows-gnu/release/soundboard.exe` (PE32+ GUI x86-64).
- Reapertura desde bandeja corregida con una señal compartida que limpia el estado oculto de eframe; compilación Linux y cross-build Windows verificados.
- Correcciones reportadas en Windows: menú conectado mediante `MenuEvent::set_event_handler`; guardado automático en `%APPDATA%\Soundboard\config.json`; selector inicia en `USERPROFILE\Music`; hotkeys aceptan los códigos físicos de corchete. Pruebas unitarias de persistencia y atajos pasan.
- Identidad visual actualizada con el icono proporcionado: se usa en la ventana y bandeja; el `.exe` de Windows incluye el recurso ICO. La interfaz tiene estilo renovado y sigue automáticamente el tema claro/oscuro del sistema.
- Verificación de esta mejora: `cargo fmt --all -- --check`, `cargo check --message-format=short`, `cargo test --bin soundboard` (5 pruebas), `cargo build --release` y `cargo build --release --target x86_64-pc-windows-gnu` satisfactorios.
- Corrección posterior a la prueba visual: las tarjetas de sonido ahora tienen ancho fijo y disposición adaptable, muestran nombre/volumen/atajo y botones visibles para editar/borrar. La bandeja ya consume `MenuEvent::receiver()` en un listener dedicado y centraliza abrir, detener y salir.
- Validación de estas correcciones: `cargo check --message-format=short`, 5 pruebas y `cargo build --release --target x86_64-pc-windows-gnu` satisfactorios; binario confirmado como PE32+ GUI x86-64.
- Corrección de la bandeja solicitada para Windows: `MenuEvent::set_event_handler` se registra antes de `eframe::run_native` con IDs de menú estables; abrir envía Visible/Minimized(false)/Focus y solicita repaint, detener y pausar actúan directamente, y salir guarda la configuración, envía Shutdown y cierra con repaint.
- Validación: `cargo fmt --all`, `cargo check --message-format=short`, 5 pruebas y nuevo `cargo build --release --target x86_64-pc-windows-gnu` satisfactorios. El menú necesita prueba interactiva en Windows.
- Archivos actualizados: `ROADMAP.md`, `Cargo.toml`, `Cargo.lock`, `build.rs`, `soundboard.png`, `soundboard.ico`, `src/app_icon.rs`, `src/main.rs`, `src/models.rs`, `src/audio/mod.rs`, `src/audio/devices.rs`, `src/audio/engine.rs`, `src/system/mod.rs`, `src/system/hotkeys.rs`, `src/system/tray.rs`, `src/ui/mod.rs`, `src/ui/app.rs`.

## Contratos e Interfaces

### Modelo persistido

- `SoundItem`: identidad, nombre visible, ruta del archivo, atajo opcional y volumen individual.
- `AudioConfig`: sonidos, volumen maestro y dispositivos seleccionados.
- `AudioDeviceSelection`: nombres opcionales para entrada, auriculares y cable virtual.

### Motor de audio

- `EngineCommand` se transporta por `crossbeam_channel::Receiver<EngineCommand>`.
- `EngineEvent` se publica por `crossbeam_channel::Sender<EngineEvent>`.
- Audio PCM predecodido y muestras activas se intercambian mediante `ringbuf::HeapRb<f32>`; el callback no debe asignar, bloquear ni llamar a APIs del sistema.
- El callback de `cpal` consumirá únicamente el extremo productor/consumidor preconfigurado y tipos atómicos.

### Sistema y UI

- La capa de sistema convierte hotkeys y eventos de bandeja en `EngineCommand`.
- La UI consume `EngineEvent` y emite `EngineCommand`; no accede directamente al callback de audio.

## Checklist de Tareas

### Fase 1 — Estructura y contratos en Rust

- [x] Crear `Cargo.toml` con las dependencias obligatorias y perfiles básicos.
- [x] Crear el punto de entrada y los módulos `audio/`, `system/`, `ui/` y `models.rs`.
- [x] Definir `SoundItem`, `AudioConfig`, selección de dispositivos y comandos/eventos compartidos.
- [x] Documentar las firmas de canales `crossbeam-channel` y buffers `ringbuf`.
- [x] Ejecutar `cargo check` de Fase 1; validación satisfactoria tras corregir errores de API en DSP y Sistema.

### Fase 2 — Delegación modular a subagentes rápidos

- [x] Subagente A: esqueleto DSP, inventario de dispositivos, decodificación/resampling y doble salida implementados en `src/audio/devices.rs`, `src/audio/decoder.rs` y `src/audio/engine.rs`.
- [x] Auditoría Arquitecto del Subagente A: callbacks usan slices tipados y `try_push`/`try_pop`; compilación confirmada en Linux.
- [x] Subagente B y auditoría de Sistema: corregidos iteradores de `cpal`, aliases/traits de `ringbuf`, mapa dinámico de hotkeys y fallback de persistencia (`XDG_CONFIG_HOME`/`HOME`/`config.json`); compilación confirmada.
- [x] Subagente C: UI eframe con cuadrícula, alta/edición/borrado, explorador local de audios, captura de hotkey, controles de dispositivo/volumen y aviso de cable implementada en `src/ui/app.rs` y exportada desde `src/ui/mod.rs`.

### Fase 3 — Auditoría y ensamblaje final

- [x] Ensamblar carga de configuración, inventario, decodificación, supervisor DSP, hotkeys, bandeja y UI en `src/main.rs`.
- [x] Conectar eventos de bandeja a la ventana desde un listener independiente; en Linux, procesar eventos GTK en el mismo hilo que crea la bandeja.
- [x] Enviar errores de callbacks de dispositivo a la UI y permitir refrescar dispositivos; el supervisor reinicia streams al cambiar la selección o detectar que un dispositivo vuelve a estar disponible.
- [x] Ejecutar `cargo check --message-format=short`: correcto, cero warnings.
- [x] Ejecutar `cargo build --release`: correcto; verificar que `target/release/soundboard` existe y es ejecutable.
- [x] Guardar configuración al salir, cerrar streams y liberar hotkeys/bandeja.

## Uso rápido

- Ejecutar en Linux: `./target/release/soundboard`.
- **Linux/PipeWire:** crear un sink virtual temporal con `pactl load-module module-null-sink sink_name=Soundboard sink_properties=device.description=Soundboard`. En Soundboard seleccionar `Soundboard` como cable virtual; en Discord/juegos elegir `Monitor of Soundboard` o `Soundboard.monitor` como micrófono. Para persistirlo, configurar el módulo null-sink en WirePlumber/PipeWire al inicio de sesión.
- **Windows:** instalar VB-CABLE. En Soundboard elegir `CABLE Input` como cable virtual y en Discord/juegos seleccionar `CABLE Output` como micrófono.
- Elegir los auriculares en Soundboard para monitorizar solo los efectos; usar **Actualizar dispositivos** después de crear/desconectar/reconectar un cable virtual.
- La reconexión automática se implementó en el supervisor mediante sondeo de dispositivos y reinicio de streams. La desconexión física no se pudo simular en esta compilación; validar con hardware real antes de distribuir.

## Identidad visual e interfaz

- [x] Usar `soundboard.png` como icono de ventana y bandeja, y generar `soundboard.ico` para incrustarlo en Windows.
- [x] Añadir estilos claros y oscuros con selección automática según el tema que notifica el sistema operativo.
- [x] Actualizar paneles, botones y tarjetas de sonidos con espaciado, bordes redondeados y colores adaptados a ambos temas.
- [x] Validar formatos, pruebas y builds de Linux y Windows.

## Siguiente Paso Exacto

Añadir una licencia y revisar el README final antes del primer commit. Crear el repositorio remoto en GitHub y publicar los cambios. Después, probar en Windows el nuevo `.exe`: confirmar que cada opción del menú de bandeja funciona con la ventana oculta; validar también Editar/Borrar, tema del sistema, persistencia, hotkeys y reconexión de dispositivos en hardware real.
