# Reglas del Proyecto: Soundboard en Rust

## Stack Técnico Obligatorio
- **Audio I/O:** `cpal` en modo compartido (WASAPI Shared / CoreAudio / PipeWire) para no bloquear juegos ni apps de voz.
- **Decodificación y Resampling:** `symphonia` (MP3, WAV, OGG, FLAC) + `rubato` (remuestreo automático al sample rate del dispositivo activo).
- **Concurrencia Lock-Free:** `ringbuf` y `crossbeam-channel`.
- **Sistema y UI:** `global-hotkey`, `tray-icon` y `serde`/`serde_json` (UI en Tauri v2 destruyendo el WebView al minimizar a bandeja, o nativa con `eframe`/`egui`).

## Reglas Inquebrantables de Código
1. **Regla de Oro del Audio (Zero-Allocation / Lock-Free):** Dentro del callback de audio de `cpal` está **estrictamente prohibido** usar `Mutex::lock()`, `.unwrap()`, `.expect()`, llamadas al sistema o asignaciones dinámicas (`Vec`, `String`, `Box`). Usa únicamente buffers `ringbuf` y tipos atómicos (`AtomicBool`, `AtomicU32`).
2. **Persistencia Continua (`ROADMAP.md`):** Al completar **cada micro-tarea**, actualiza `ROADMAP.md` marcando `- [x]`, registrando los archivos tocados y actualizando la sección `## Siguiente Paso Exacto`.
3. **Ahorro de Contexto:** Lee y edita únicamente los archivos asignados a tu tarea actual. No reescribas módulos ya marcados como completados en `ROADMAP.md`.