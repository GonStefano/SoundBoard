#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app_icon;
mod audio;
mod models;
mod system;
mod ui;

use std::collections::HashMap;
use std::path::Path;
#[cfg_attr(not(windows), allow(unused_imports))]
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW,
};

/// HWND nativo de la ventana principal; se captura en el primer frame.
/// Solo se usa en Windows para mostrar/ocultar sin pasar por egui.
#[cfg(windows)]
static NATIVE_HWND: AtomicIsize = AtomicIsize::new(0);

use audio::{decode_and_resample, find_output_device, AudioEngine, DecodedSound, DeviceInventory};
use cpal::traits::{DeviceTrait, HostTrait};
use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use models::{AudioConfig, AudioDeviceSelection, EngineCommand, EngineEvent, SoundItem};
use system::{load_config, save_config, GlobalHotkeyController, SystemTray, TrayMenuIds};
use tray_icon::menu::MenuEvent;
use ui::{SoundboardApp, UiDeviceLists};

fn main() {
    if let Err(error) = run() {
        eprintln!("Soundboard no pudo iniciarse: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = load_config(None);
    let inventory = DeviceInventory::refresh(&cpal::default_host());
    if !inventory.virtual_cable_detected() {
        eprintln!("Aviso: no se detectó un cable virtual de salida.");
    }

    let ui_devices = make_ui_device_lists(&inventory);
    let (command_sender, command_receiver) = crossbeam_channel::unbounded();
    let (event_sender, event_receiver) = crossbeam_channel::unbounded();
    let audio_supervisor = start_audio_supervisor(command_receiver, event_sender, config.clone())?;
    let hotkeys = GlobalHotkeyController::new(&config.sounds, command_sender.clone())
        .map_err(|error| format!("No se pudieron inicializar los hotkeys: {error}"))?;
    let pause_state = hotkeys.pause_handle();
    let hotkeys = Arc::new(Mutex::new(hotkeys));
    let exit_requested = Arc::new(AtomicBool::new(false));
    let wake_requested = Arc::new(AtomicBool::new(false));
    let shared_config = Arc::new(Mutex::new(config.clone()));
    let tray_context = Arc::new(Mutex::new(None));
    let tray_menu_ids = TrayMenuIds::new();
    register_tray_menu_handler(
        &tray_menu_ids,
        tray_context.clone(),
        command_sender.clone(),
        pause_state.clone(),
        exit_requested.clone(),
        wake_requested.clone(),
        shared_config.clone(),
    );

    #[cfg(target_os = "linux")]
    let mut linux_tray = start_linux_tray(tray_menu_ids.clone())?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([900.0, 680.0])
        .with_min_inner_size([640.0, 480.0]);
    match app_icon::rgba_icon(256) {
        Ok((rgba, width, height)) => {
            viewport = viewport.with_icon(egui::IconData {
                rgba,
                width,
                height,
            });
        }
        Err(error) => eprintln!("Aviso: no se pudo cargar el icono de la aplicación: {error}"),
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let app_commands = command_sender.clone();
    let app_config = config.clone();
    let app_devices = ui_devices.clone();
    let app_events = event_receiver.clone();
    let app_hotkeys = hotkeys.clone();
    let app_exit_requested = exit_requested.clone();
    let app_wake_requested = wake_requested.clone();
    let app_tray_context = tray_context.clone();
    let app_shared_config = shared_config.clone();
    #[cfg(not(target_os = "linux"))]
    let app_tray_menu_ids = tray_menu_ids;

    let result = eframe::run_native(
        "Soundboard",
        native_options,
        Box::new(move |creation_context| {
            match app_tray_context.lock() {
                Ok(mut context) => *context = Some(creation_context.egui_ctx.clone()),
                Err(error) => {
                    return Err(std::io::Error::other(format!(
                        "No se pudo inicializar el contexto de la bandeja: {error}"
                    ))
                    .into());
                }
            }

            #[cfg(target_os = "linux")]
            let tray_owner = None;

            #[cfg(not(target_os = "linux"))]
            let tray_owner = Some(
                SystemTray::new(&app_tray_menu_ids)
                    .map_err(|error| std::io::Error::other(error))?,
            );

            let soundboard = SoundboardApp::new(
                app_config.clone(),
                app_commands.clone(),
                app_events.clone(),
                app_devices.clone(),
                app_exit_requested.clone(),
                app_wake_requested.clone(),
            );
            let sound_hotkeys = sound_hotkey_map(&app_config.sounds);

            Ok(Box::new(RuntimeApp {
                soundboard,
                commands: app_commands.clone(),
                hotkeys: app_hotkeys.clone(),
                last_sound_hotkeys: sound_hotkeys,
                exit_requested: app_exit_requested.clone(),
                shared_config: app_shared_config.clone(),
                last_saved_config: app_config.clone(),
                config_dirty_since: None,
                _tray_owner: tray_owner,
                audio_supervisor: Some(audio_supervisor),
                #[cfg(target_os = "linux")]
                _linux_tray: linux_tray.take(),
            }))
        }),
    );

    result.map_err(|error| error.to_string())
}

struct RuntimeApp {
    soundboard: SoundboardApp,
    commands: Sender<EngineCommand>,
    hotkeys: Arc<Mutex<GlobalHotkeyController>>,
    last_sound_hotkeys: HashMap<u64, Option<String>>,
    exit_requested: Arc<AtomicBool>,
    shared_config: Arc<Mutex<AudioConfig>>,
    last_saved_config: AudioConfig,
    config_dirty_since: Option<Instant>,
    _tray_owner: Option<SystemTray>,
    audio_supervisor: Option<JoinHandle<()>>,
    #[cfg(target_os = "linux")]
    _linux_tray: Option<LinuxTrayLoop>,
}

impl eframe::App for RuntimeApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Captura el HWND nativo en el primer frame (solo Windows).
        // HWND en windows 0.58 envuelve *mut c_void; lo almacenamos como isize
        // (puntero opaco) para poder cruzar hilos con un AtomicIsize.
        #[cfg(windows)]
        if NATIVE_HWND.load(Ordering::Acquire) == 0 {
            if let Ok(handle) = frame.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    NATIVE_HWND.store(h.hwnd.get(), Ordering::Release);
                }
            }
        }

        let previous_sounds = self.soundboard.config().sounds.clone();
        #[cfg(windows)]
        let prev_hidden = self.soundboard.is_hidden();
        self.soundboard.update(ctx, frame);

        // Detecta cambios de visibilidad y llama a ShowWindow en Windows,
        // donde ViewportCommand::Visible tiene el bug documentado #737/#5127.
        #[cfg(windows)]
        {
            let now_hidden = self.soundboard.is_hidden();
            if prev_hidden != now_hidden {
                let hwnd = NATIVE_HWND.load(Ordering::Acquire);
                if hwnd != 0 {
                    unsafe {
                        ShowWindow(HWND(hwnd as *mut core::ffi::c_void), if now_hidden { SW_HIDE } else { SW_SHOW });
                    }
                }
            }
        }

        let config = self.soundboard.config().clone();
        match self.shared_config.lock() {
            Ok(mut shared_config) => *shared_config = config.clone(),
            Err(error) => {
                eprintln!("No se pudo actualizar la configuración compartida: {error}");
            }
        }
        if same_config(&config, &self.last_saved_config) {
            self.config_dirty_since = None;
        } else {
            let dirty_since = self.config_dirty_since.get_or_insert_with(Instant::now);
            if self.soundboard.is_hidden()
                || self.exit_requested.load(Ordering::Acquire)
                || dirty_since.elapsed() >= Duration::from_millis(500)
            {
                match save_config(None, &config) {
                    Ok(_) => {
                        self.last_saved_config = config.clone();
                        self.config_dirty_since = None;
                    }
                    Err(error) => {
                        eprintln!("No se pudo guardar automáticamente la configuración: {error}");
                        self.config_dirty_since = Some(Instant::now());
                    }
                }
            }
        }

        let hotkey_map = sound_hotkey_map(&config.sounds);
        if hotkey_map != self.last_sound_hotkeys {
            if let Ok(mut hotkeys) = self.hotkeys.lock() {
                for sound_id in self.last_sound_hotkeys.keys() {
                    if !hotkey_map.contains_key(sound_id) {
                        hotkeys.unregister(*sound_id);
                    }
                }
                for sound in &config.sounds {
                    if self.last_sound_hotkeys.get(&sound.id) != hotkey_map.get(&sound.id) {
                        if !hotkeys.register_dynamic(sound) {
                            eprintln!(
                                "No se pudo registrar el hotkey del sonido '{}'.",
                                sound.name
                            );
                        }
                    }
                }
            }
            self.last_sound_hotkeys = hotkey_map;
        }

        if !same_sounds(&previous_sounds, &config.sounds) {
            if self
                .commands
                .try_send(EngineCommand::ReloadSounds {
                    sounds: config.sounds.clone(),
                })
                .is_err()
            {
                eprintln!("No se pudo recargar la biblioteca de sonidos.");
            }
        }

        if self.exit_requested.load(Ordering::Acquire) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint();
        }
    }
}

impl Drop for RuntimeApp {
    fn drop(&mut self) {
        if let Err(error) = save_config(None, self.soundboard.config()) {
            eprintln!("No se pudo guardar la configuración: {error}");
        }

        let _ = self.commands.try_send(EngineCommand::Shutdown);
        if let Ok(mut hotkeys) = self.hotkeys.lock() {
            hotkeys.clear();
        }

        if let Some(supervisor) = self.audio_supervisor.take() {
            let _ = supervisor.join();
        }
    }
}

fn sound_hotkey_map(sounds: &[SoundItem]) -> HashMap<u64, Option<String>> {
    sounds
        .iter()
        .map(|sound| (sound.id, sound.hotkey.clone()))
        .collect()
}

fn same_sounds(left: &[SoundItem], right: &[SoundItem]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.id == right.id
                && left.name == right.name
                && left.file_path == right.file_path
                && left.hotkey == right.hotkey
                && left.volume.to_bits() == right.volume.to_bits()
        })
}

fn same_config(left: &AudioConfig, right: &AudioConfig) -> bool {
    left.master_volume.to_bits() == right.master_volume.to_bits()
        && left.devices.input == right.devices.input
        && left.devices.headphones == right.devices.headphones
        && left.devices.virtual_microphone == right.devices.virtual_microphone
        && same_sounds(&left.sounds, &right.sounds)
}

fn make_ui_device_lists(inventory: &DeviceInventory) -> UiDeviceLists {
    let is_virtual = |name: &str| {
        let name = name.to_ascii_lowercase();
        name.contains("cable")
            || name.contains("blackhole")
            || name.contains("virtual")
            || (name.contains("pipewire") && name.contains("virtual"))
    };

    UiDeviceLists {
        microphones: inventory.inputs.clone(),
        virtual_outputs: inventory
            .outputs
            .iter()
            .filter(|name| is_virtual(name))
            .cloned()
            .collect(),
        headphones: inventory
            .outputs
            .iter()
            .filter(|name| !is_virtual(name))
            .cloned()
            .collect(),
    }
}

fn start_audio_supervisor(
    commands: Receiver<EngineCommand>,
    events: Sender<EngineEvent>,
    config: AudioConfig,
) -> Result<JoinHandle<()>, String> {
    let (ready_sender, ready_receiver) = crossbeam_channel::bounded(1);
    let supervisor = thread::spawn(move || {
        audio_supervisor_loop(commands, events, config, ready_sender);
    });

    match ready_receiver.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(())) => Ok(supervisor),
        Ok(Err(error)) => {
            let _ = supervisor.join();
            Err(error)
        }
        Err(error) => Err(format!(
            "El motor de audio no respondió al iniciar: {error}"
        )),
    }
}

fn audio_supervisor_loop(
    commands: Receiver<EngineCommand>,
    events: Sender<EngineEvent>,
    mut config: AudioConfig,
    ready: Sender<Result<(), String>>,
) {
    let mut startup = true;
    loop {
        let sound_bank = load_sound_bank(&config, &events);
        let (engine_commands, engine_receiver) = crossbeam_channel::unbounded();
        let engine = AudioEngine::start(
            engine_receiver,
            events.clone(),
            config.devices.clone(),
            Arc::new(sound_bank),
        );

        let engine = match engine {
            Ok(engine) => engine,
            Err(error) => {
                let _ = events.try_send(EngineEvent::Error(error.clone()));
                if startup {
                    let _ = ready.try_send(Err(error));
                    return;
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let is_startup = startup;
        startup = false;
        let _ = engine_commands.try_send(EngineCommand::SetMasterVolume {
            volume: config.master_volume,
        });

        if is_startup {
            let _ = ready.try_send(Ok(()));
        }

        let mut was_available = selected_devices_available(&config.devices);
        loop {
            match commands.recv_timeout(Duration::from_secs(2)) {
                Ok(EngineCommand::SetDevices(selection)) => {
                    config.devices = selection;
                    break;
                }
                Ok(EngineCommand::ReloadSounds { sounds }) => {
                    config.sounds = sounds;
                    break;
                }
                Ok(EngineCommand::Shutdown) => {
                    let _ = engine_commands.try_send(EngineCommand::Shutdown);
                    drop(engine);
                    return;
                }
                Ok(command) => {
                    let _ = engine_commands.try_send(command);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    let available = selected_devices_available(&config.devices);
                    if available && !was_available {
                        break;
                    }
                    was_available = available;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    let _ = engine_commands.try_send(EngineCommand::Shutdown);
                    drop(engine);
                    return;
                }
            }
        }

        let _ = engine_commands.try_send(EngineCommand::Shutdown);
        drop(engine);
    }
}

fn load_sound_bank(config: &AudioConfig, events: &Sender<EngineEvent>) -> Vec<DecodedSound> {
    let host = cpal::default_host();
    let output_device = find_output_device(
        &host,
        config
            .devices
            .virtual_microphone
            .as_deref()
            .or(config.devices.headphones.as_deref()),
    )
    .or_else(|| host.default_output_device());
    let sample_rate = output_device
        .and_then(|device| device.default_output_config().ok())
        .map(|supported| supported.sample_rate().0)
        .unwrap_or(48_000);

    config
        .sounds
        .iter()
        .filter_map(|sound| {
            match decode_and_resample(
                sound.id,
                sound.volume,
                Path::new(&sound.file_path),
                sample_rate,
            ) {
                Ok(decoded) => Some(decoded),
                Err(error) => {
                    let _ = events.try_send(EngineEvent::Error(format!(
                        "No se pudo cargar '{}': {error}",
                        sound.name
                    )));
                    None
                }
            }
        })
        .collect()
}

fn selected_devices_available(selection: &AudioDeviceSelection) -> bool {
    let host = cpal::default_host();
    let inputs = host
        .input_devices()
        .map(|devices| {
            devices
                .filter_map(|device| device.name().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let outputs = host
        .output_devices()
        .map(|devices| {
            devices
                .filter_map(|device| device.name().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    selection.input.as_ref().map_or_else(
        || host.default_input_device().is_some(),
        |name| inputs.contains(name),
    ) && selection.headphones.as_ref().map_or_else(
        || host.default_output_device().is_some(),
        |name| outputs.contains(name),
    ) && selection.virtual_microphone.as_ref().map_or_else(
        || host.default_output_device().is_some(),
        |name| outputs.contains(name),
    )
}

fn register_tray_menu_handler(
    ids: &TrayMenuIds,
    context: Arc<Mutex<Option<egui::Context>>>,
    commands: Sender<EngineCommand>,
    paused: Arc<AtomicBool>,
    exit_requested: Arc<AtomicBool>,
    wake_requested: Arc<AtomicBool>,
    shared_config: Arc<Mutex<AudioConfig>>,
) {
    let open_id = ids.open.clone();
    let toggle_hotkeys_id = ids.toggle_hotkeys.clone();
    let stop_all_id = ids.stop_all.clone();
    let quit_id = ids.quit.clone();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            wake_requested.store(true, Ordering::Release);
            // En Windows usamos la API Win32 directamente para evitar el bug
            // de egui/eframe donde ViewportCommand::Visible(true) no funciona
            // con la ventana previamente oculta con SW_HIDE (issues #737, #5127).
            #[cfg(windows)]
            {
                let hwnd = NATIVE_HWND.load(Ordering::Acquire);
                if hwnd != 0 {
                    unsafe {
                        ShowWindow(HWND(hwnd as *mut core::ffi::c_void), SW_SHOW);
                        ShowWindow(HWND(hwnd as *mut core::ffi::c_void), SW_RESTORE);
                        let _ = SetForegroundWindow(HWND(hwnd as *mut core::ffi::c_void));
                    }
                }
            }
            #[cfg(not(windows))]
            match context.lock() {
                Ok(context) => {
                    if let Some(context) = context.as_ref() {
                        context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        context.send_viewport_cmd(egui::ViewportCommand::Focus);
                        context.request_repaint();
                    } else {
                        eprintln!(
                            "No se pudo abrir la ventana: el contexto de eframe aún no está listo."
                        );
                    }
                }
                Err(error) => {
                    eprintln!("No se pudo acceder al contexto de eframe: {error}");
                }
            }
            match context.lock() {
                Ok(ctx) => {
                    if let Some(ctx) = ctx.as_ref() {
                        ctx.request_repaint();
                    }
                }
                Err(_) => {}
            }
        } else if event.id == toggle_hotkeys_id {
            paused.fetch_xor(true, Ordering::AcqRel);
        } else if event.id == stop_all_id {
            if commands.try_send(EngineCommand::StopAll).is_err() {
                eprintln!("No se pudo detener el audio desde la bandeja.");
            }
        } else if event.id == quit_id {
            exit_requested.store(true, Ordering::Release);
            match shared_config.lock() {
                Ok(config) => {
                    if let Err(error) = save_config(None, &config) {
                        eprintln!("No se pudo guardar la configuración al salir: {error}");
                    }
                }
                Err(error) => {
                    eprintln!("No se pudo leer la configuración al salir: {error}");
                }
            }
            if commands.try_send(EngineCommand::Shutdown).is_err() {
                eprintln!("No se pudo enviar el cierre al motor de audio.");
            }
            // En Windows, cuando la ventana está oculta eframe no procesa
            // ViewportCommand::Close. Salimos directamente tras dar tiempo al
            // motor de audio para cerrar los streams (~200 ms).
            #[cfg(windows)]
            {
                std::thread::sleep(std::time::Duration::from_millis(200));
                std::process::exit(0);
            }
            #[cfg(not(windows))]
            match context.lock() {
                Ok(context) => {
                    if let Some(context) = context.as_ref() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                        context.request_repaint();
                    } else {
                        eprintln!("No se pudo cerrar la ventana: el contexto de eframe aún no está listo.");
                    }
                }
                Err(error) => {
                    eprintln!("No se pudo acceder al contexto de eframe: {error}");
                }
            }
        }
    }));
}

#[cfg(target_os = "linux")]
struct LinuxTrayLoop {
    stop_sender: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Drop for LinuxTrayLoop {
    fn drop(&mut self) {
        let _ = self.stop_sender.try_send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn start_linux_tray(ids: TrayMenuIds) -> Result<Option<LinuxTrayLoop>, String> {
    let (ready_sender, ready_receiver) = crossbeam_channel::bounded(1);
    let (stop_sender, stop_receiver) = crossbeam_channel::bounded(1);
    let tray_thread = thread::spawn(move || {
        if let Err(error) = gtk::init() {
            let _ = ready_sender.try_send(Err(format!("GTK no disponible: {error}")));
            return;
        }

        let tray = match SystemTray::new(&ids) {
            Ok(tray) => tray,
            Err(error) => {
                let _ = ready_sender.try_send(Err(error));
                return;
            }
        };
        if ready_sender.try_send(Ok(())).is_err() {
            return;
        }

        loop {
            if stop_receiver.try_recv().is_ok() {
                break;
            }
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            thread::sleep(Duration::from_millis(10));
        }
        drop(tray);
    });

    match ready_receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(Some(LinuxTrayLoop {
            stop_sender,
            thread: Some(tray_thread),
        })),
        Ok(Err(error)) => {
            eprintln!("Aviso: la bandeja del sistema no está disponible: {error}");
            let _ = tray_thread.join();
            Ok(None)
        }
        Err(error) => {
            let _ = stop_sender.try_send(());
            let _ = tray_thread.join();
            Err(format!(
                "No se pudo iniciar el bucle GTK de bandeja: {error}"
            ))
        }
    }
}
