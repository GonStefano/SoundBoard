use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use eframe::egui::{self, Event, Key, Modifiers, RichText};

use crate::models::{AudioConfig, EngineCommand, EngineEvent, SoundId, SoundItem};

#[derive(Debug, Clone, Default)]
pub struct UiDeviceLists {
    pub microphones: Vec<String>,
    pub virtual_outputs: Vec<String>,
    pub headphones: Vec<String>,
}

struct SoundEditor {
    id: Option<SoundId>,
    name: String,
    file_path: String,
    hotkey: String,
    volume: f32,
    capturing_hotkey: bool,
}

impl SoundEditor {
    fn new(sound: Option<&SoundItem>) -> Self {
        match sound {
            Some(sound) => Self {
                id: Some(sound.id),
                name: sound.name.clone(),
                file_path: sound.file_path.clone(),
                hotkey: sound.hotkey.clone().unwrap_or_default(),
                volume: sound.volume.clamp(0.0, 1.0),
                capturing_hotkey: false,
            },
            None => Self {
                id: None,
                name: String::new(),
                file_path: String::new(),
                hotkey: String::new(),
                volume: 1.0,
                capturing_hotkey: false,
            },
        }
    }

    fn to_sound(&self, id: SoundId) -> SoundItem {
        SoundItem {
            id: self.id.unwrap_or(id),
            name: self.name.trim().to_string(),
            file_path: self.file_path.trim().to_string(),
            hotkey: if self.hotkey.trim().is_empty() {
                None
            } else {
                Some(self.hotkey.trim().to_string())
            },
            volume: self.volume.clamp(0.0, 1.0),
        }
    }
}

pub struct SoundboardApp {
    config: AudioConfig,
    commands: Sender<EngineCommand>,
    events: Receiver<EngineEvent>,
    devices: UiDeviceLists,
    editor: Option<SoundEditor>,
    next_id: SoundId,
    status: Option<String>,
    virtual_cable_available: bool,
    hidden: bool,
    file_picker_open: bool,
    file_picker_directory: PathBuf,
    exit_requested: Arc<AtomicBool>,
    wake_requested: Arc<AtomicBool>,
    system_theme_initialized: bool,
    styled_dark_mode: Option<bool>,
}

impl SoundboardApp {
    pub fn new(
        config: AudioConfig,
        commands: Sender<EngineCommand>,
        events: Receiver<EngineEvent>,
        devices: UiDeviceLists,
        exit_requested: Arc<AtomicBool>,
        wake_requested: Arc<AtomicBool>,
    ) -> Self {
        let next_id = config
            .sounds
            .iter()
            .map(|sound| sound.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let virtual_cable_available = !devices.virtual_outputs.is_empty();

        Self {
            config,
            commands,
            events,
            devices,
            editor: None,
            next_id,
            status: None,
            virtual_cable_available,
            hidden: false,
            file_picker_open: false,
            file_picker_directory: default_music_directory(),
            exit_requested,
            wake_requested,
            system_theme_initialized: false,
            styled_dark_mode: None,
        }
    }

    pub fn config(&self) -> &AudioConfig {
        &self.config
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    pub fn set_visible(&mut self, ctx: &egui::Context, visible: bool) {
        self.hidden = !visible;
        // En Windows la visibilidad real la gestiona RuntimeApp vía ShowWindow
        // (Win32) para evitar el bug de egui #737/#5127 con ViewportCommand::Visible.
        #[cfg(not(windows))]
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(visible));
            if visible {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
            }
        }
        #[cfg(windows)]
        let _ = ctx; // RuntimeApp::update detecta el cambio de hidden y llama ShowWindow
    }

    fn send_command(&mut self, command: EngineCommand) {
        if self.commands.try_send(command).is_err() {
            self.status = Some("No se pudo enviar el comando al motor de audio.".to_string());
        }
    }

    fn process_engine_events(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(EngineEvent::VirtualCableUnavailable) => {
                    self.virtual_cable_available = false;
                }
                Ok(EngineEvent::Error(message)) => self.status = Some(message),
                Ok(EngineEvent::InputDeviceUnavailable) => {
                    self.status = Some("No se detecta un micrófono de entrada.".to_string());
                }
                Ok(EngineEvent::OutputDeviceUnavailable) => {
                    self.status = Some("No se detecta un dispositivo de salida.".to_string());
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    fn select_sound_file(&mut self, path: &Path) {
        if !is_supported_audio(path) {
            self.status = Some("Formato no compatible. Usa MP3, WAV, OGG o FLAC.".to_string());
            return;
        }
        if let Some(editor) = &mut self.editor {
            editor.file_path = path.to_string_lossy().into_owned();
            if editor.name.trim().is_empty() {
                editor.name = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_string();
            }
        }
    }

    fn dropped_audio_path(&self, ctx: &egui::Context) -> Option<String> {
        ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .find_map(|file| file.path.as_deref().filter(|path| is_supported_audio(path)))
                .and_then(|path| path.to_str())
                .map(str::to_string)
        })
    }

    fn capture_hotkey(&mut self, ctx: &egui::Context) {
        let Some(editor) = self
            .editor
            .as_mut()
            .filter(|editor| editor.capturing_hotkey)
        else {
            return;
        };

        let captured = ctx.input(|input| {
            input.raw.events.iter().find_map(|event| match event {
                Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => format_hotkey(*key, *modifiers),
                _ => None,
            })
        });

        if let Some(hotkey) = captured {
            editor.hotkey = hotkey;
            editor.capturing_hotkey = false;
        }
    }

    fn draw_device_controls(&mut self, ui: &mut egui::Ui) {
        let old_selection = self.config.devices.clone();
        egui::Grid::new("audio-device-selectors")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Micrófono real");
                device_combo(
                    ui,
                    "device-input",
                    &self.devices.microphones,
                    &mut self.config.devices.input,
                    "Predeterminado",
                );
                ui.end_row();

                ui.label("Cable virtual (Salida B)");
                device_combo(
                    ui,
                    "device-virtual-output",
                    &self.devices.virtual_outputs,
                    &mut self.config.devices.virtual_microphone,
                    "Selecciona un cable",
                );
                ui.end_row();

                ui.label("Auriculares (Monitorización A)");
                device_combo(
                    ui,
                    "device-headphones",
                    &self.devices.headphones,
                    &mut self.config.devices.headphones,
                    "Predeterminado",
                );
                ui.end_row();
            });

        if old_selection.input != self.config.devices.input
            || old_selection.headphones != self.config.devices.headphones
            || old_selection.virtual_microphone != self.config.devices.virtual_microphone
        {
            self.send_command(EngineCommand::SetDevices(self.config.devices.clone()));
        }

        if ui.button("Actualizar dispositivos").clicked() {
            self.refresh_devices();
        }
    }

    fn refresh_devices(&mut self) {
        let host = cpal::default_host();
        let inputs = match enumerate_inputs(&host) {
            Ok(inputs) => inputs,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };
        let outputs = match enumerate_outputs(&host) {
            Ok(outputs) => outputs,
            Err(error) => {
                self.status = Some(error);
                return;
            }
        };

        self.devices.microphones = inputs;
        self.devices.virtual_outputs = outputs
            .iter()
            .filter(|name| is_virtual_output(name))
            .cloned()
            .collect();
        self.devices.headphones = outputs
            .into_iter()
            .filter(|name| !is_virtual_output(name))
            .collect();
        self.virtual_cable_available = !self.devices.virtual_outputs.is_empty();
    }

    fn draw_sound_grid(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Biblioteca de sonidos");
            ui.label(
                egui::RichText::new(format!("{} efectos", self.config.sounds.len()))
                    .small()
                    .weak(),
            );
            if ui.button("+ Añadir audio").clicked() {
                self.editor = Some(SoundEditor::new(None));
            }
        });
        ui.add_space(8.0);

        let mut play_id = None;
        let mut edit_id = None;
        let mut delete_id = None;

        let dark_mode = ui.visuals().dark_mode;
        let button_fill = if dark_mode {
            egui::Color32::from_rgb(55, 69, 124)
        } else {
            egui::Color32::from_rgb(225, 231, 255)
        };
        let button_text = if dark_mode {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(42, 52, 112)
        };
        let card_fill = ui.visuals().faint_bg_color;
        ui.horizontal_wrapped(|ui| {
            for sound in &self.config.sounds {
                egui::Frame::group(ui.style())
                    .fill(card_fill)
                    .rounding(14.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_width(250.0);
                        let name = if sound.name.trim().is_empty() {
                            "Sonido sin nombre"
                        } else {
                            sound.name.as_str()
                        };
                        if ui
                            .add_sized(
                                [226.0, 56.0],
                                egui::Button::new(
                                    RichText::new(format!("▶  {name}"))
                                        .strong()
                                        .color(button_text),
                                )
                                .fill(button_fill)
                                .rounding(10.0),
                            )
                            .clicked()
                        {
                            play_id = Some(sound.id);
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.small(format!("Volumen {:.0}%", sound.volume * 100.0));
                            if let Some(hotkey) = &sound.hotkey {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(RichText::new(hotkey).small().monospace().weak());
                                    },
                                );
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Editar").clicked() {
                                edit_id = Some(sound.id);
                            }
                            if ui.button("Borrar").clicked() {
                                delete_id = Some(sound.id);
                            }
                        });
                    });
            }
        });

        if let Some(sound_id) = play_id {
            self.send_command(EngineCommand::PlaySound { sound_id });
        }
        if let Some(sound_id) = edit_id {
            if let Some(sound) = self.config.sounds.iter().find(|sound| sound.id == sound_id) {
                self.editor = Some(SoundEditor::new(Some(sound)));
            }
        }
        if let Some(sound_id) = delete_id {
            self.config.sounds.retain(|sound| sound.id != sound_id);
        }
    }

    fn draw_editor(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.editor.take() else {
            return;
        };

        let mut keep_open = true;
        let mut save_editor = false;
        let mut cancel_editor = false;
        let title = if editor.id.is_some() {
            "Editar sonido"
        } else {
            "Añadir sonido"
        };

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut keep_open)
            .show(ctx, |ui| {
                ui.label("Archivo de audio (MP3, WAV, OGG o FLAC)");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut editor.file_path);
                    if ui.button("Examinar...").clicked() {
                        self.file_picker_open = true;
                    }
                });
                ui.small("También puedes arrastrar un archivo de audio a la ventana.");

                ui.label("Nombre");
                ui.text_edit_singleline(&mut editor.name);

                ui.horizontal(|ui| {
                    ui.label("Atajo global");
                    ui.text_edit_singleline(&mut editor.hotkey);
                    let button = if editor.capturing_hotkey {
                        "Pulsa una combinación..."
                    } else {
                        "Capturar"
                    };
                    if ui.button(button).clicked() {
                        editor.capturing_hotkey = true;
                    }
                });

                ui.add(
                    egui::Slider::new(&mut editor.volume, 0.0..=1.0)
                        .text("Volumen individual")
                        .suffix(""),
                );

                ui.horizontal(|ui| {
                    if ui.button("Guardar").clicked() {
                        save_editor = true;
                    }
                    if ui.button("Cancelar").clicked() {
                        cancel_editor = true;
                    }
                });
            });

        if cancel_editor {
            keep_open = false;
        }
        if save_editor {
            if editor.name.trim().is_empty() || editor.file_path.trim().is_empty() {
                self.status = Some("El nombre y la ruta del audio son obligatorios.".to_string());
                keep_open = true;
            } else if !is_supported_audio(Path::new(editor.file_path.trim())) {
                self.status = Some("Selecciona un archivo MP3, WAV, OGG o FLAC.".to_string());
                keep_open = true;
            } else {
                let sound = editor.to_sound(self.next_id);
                if sound.id == self.next_id {
                    self.next_id = self.next_id.saturating_add(1);
                    self.config.sounds.push(sound);
                } else if let Some(existing) = self
                    .config
                    .sounds
                    .iter_mut()
                    .find(|existing| existing.id == sound.id)
                {
                    *existing = sound;
                }
                keep_open = false;
            }
        }

        if keep_open {
            self.editor = Some(editor);
        }
        self.draw_file_picker(ctx);
    }

    fn draw_file_picker(&mut self, ctx: &egui::Context) {
        if !self.file_picker_open {
            return;
        }

        let mut open = true;
        let mut selected_file = None;
        let mut navigate_to = None;
        egui::Window::new("Seleccionar archivo de audio")
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(self.file_picker_directory.display().to_string());
                if ui.button("Subir carpeta").clicked() {
                    navigate_to = self.file_picker_directory.parent().map(Path::to_path_buf);
                }

                ui.separator();
                egui::ScrollArea::vertical().max_height(280.0).show(
                    ui,
                    |ui| match std::fs::read_dir(&self.file_picker_directory) {
                        Ok(entries) => {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_dir() {
                                    let label = format!(
                                        "📁 {}",
                                        path.file_name()
                                            .and_then(|name| name.to_str())
                                            .unwrap_or("(carpeta)")
                                    );
                                    if ui.button(label).clicked() {
                                        navigate_to = Some(path);
                                    }
                                } else if is_supported_audio(&path) {
                                    let label = path
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("(audio)")
                                        .to_string();
                                    if ui.selectable_label(false, label).clicked() {
                                        selected_file = Some(path);
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            ui.colored_label(
                                egui::Color32::LIGHT_RED,
                                format!("No se pudo leer la carpeta: {error}"),
                            );
                        }
                    },
                );
            });

        if let Some(directory) = navigate_to {
            self.file_picker_directory = directory;
        }
        if let Some(path) = selected_file {
            self.select_sound_file(&path);
            self.file_picker_open = false;
        } else {
            self.file_picker_open = open;
        }
    }
}

impl eframe::App for SoundboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.system_theme_initialized {
            ctx.set_theme(egui::ThemePreference::System);
            self.system_theme_initialized = true;
        }
        let dark_mode = ctx.style().visuals.dark_mode;
        if self.styled_dark_mode != Some(dark_mode) {
            apply_modern_style(ctx);
            self.styled_dark_mode = Some(dark_mode);
        }

        self.process_engine_events();

        if self.wake_requested.swap(false, Ordering::AcqRel) {
            self.hidden = false;
            // En Windows RuntimeApp ya llamó a ShowWindow(SW_SHOW) desde el
            // tray handler; solo actualizamos el estado interno y forzamos repaint.
            #[cfg(not(windows))]
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            ctx.request_repaint();
        }

        if ctx.input(|input| input.viewport().close_requested()) {
            if !self.exit_requested.load(Ordering::Acquire) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.set_visible(ctx, false);
            }
        }
        if self.hidden {
            ctx.request_repaint_after(Duration::from_secs(3600));
            return;
        }

        if let Some(path) = self.dropped_audio_path(ctx) {
            if self.editor.is_none() {
                self.editor = Some(SoundEditor::new(None));
            }
            self.select_sound_file(Path::new(&path));
        }
        self.capture_hotkey(ctx);

        egui::TopBottomPanel::top("top-bar").show(ctx, |ui| {
            egui::Frame::none()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(egui::Margin::symmetric(18.0, 12.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.heading(RichText::new("Soundboard").size(23.0).strong());
                            ui.label(RichText::new("Tu colección de efectos").small().weak());
                        });
                        ui.separator();
                        ui.small(if dark_mode {
                            "Tema del sistema · Oscuro"
                        } else {
                            "Tema del sistema · Claro"
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Detener todos los sonidos").strong(),
                                    )
                                    .fill(egui::Color32::from_rgb(173, 55, 66))
                                    .rounding(10.0),
                                )
                                .clicked()
                            {
                                self.send_command(EngineCommand::StopAll);
                            }
                            if ui.button("Ocultar").clicked() {
                                self.set_visible(ctx, false);
                            }
                        });
                    });
                });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.add_space(8.0);
                ui.heading("Configuración de audio");
                ui.label(
                    RichText::new("Elige el micrófono, la salida virtual y tus auriculares.")
                        .weak(),
                );
                ui.add_space(10.0);
                egui::Frame::group(ui.style())
                    .fill(ui.visuals().faint_bg_color)
                    .rounding(14.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.heading("Dispositivos");
                        ui.add_space(6.0);
                        self.draw_device_controls(ui);
                    });

                if !self.virtual_cable_available {
                    ui.add_space(8.0);
                    egui::Frame::group(ui.style())
                        .fill(egui::Color32::from_rgb(100, 45, 36))
                        .rounding(10.0)
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.colored_label(
                                egui::Color32::WHITE,
                                "No se detecta un cable virtual. Configura VB-Cable, BlackHole o PipeWire Virtual.",
                            );
                        });
                }

                ui.add_space(10.0);
                egui::Frame::group(ui.style())
                    .fill(ui.visuals().faint_bg_color)
                    .rounding(14.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading("Volumen maestro");
                            let response = ui.add(
                                egui::Slider::new(&mut self.config.master_volume, 0.0..=1.0)
                                    .show_value(true)
                                    .text("Nivel"),
                            );
                            if response.changed() {
                                self.send_command(EngineCommand::SetMasterVolume {
                                    volume: self.config.master_volume,
                                });
                            }
                        });
                    });

                ui.separator();
                self.draw_sound_grid(ui);

                if let Some(status) = &self.status {
                    ui.separator();
                    ui.colored_label(egui::Color32::YELLOW, status);
                }
            });
        });

        self.draw_editor(ctx);
    }
}

fn apply_modern_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let dark_mode = style.visuals.dark_mode;
    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(18.0);
    style.visuals.window_rounding = egui::Rounding::same(14.0);
    style.visuals.menu_rounding = egui::Rounding::same(12.0);
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(9.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(9.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(9.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(9.0);
    style.visuals.selection.bg_fill = accent_color(dark_mode);
    style.visuals.hyperlink_color = accent_color(dark_mode);

    if dark_mode {
        style.visuals.panel_fill = egui::Color32::from_rgb(20, 23, 31);
        style.visuals.window_fill = egui::Color32::from_rgb(27, 31, 41);
        style.visuals.faint_bg_color = egui::Color32::from_rgb(34, 39, 51);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(15, 18, 24);
    } else {
        style.visuals.panel_fill = egui::Color32::from_rgb(245, 247, 251);
        style.visuals.window_fill = egui::Color32::WHITE;
        style.visuals.faint_bg_color = egui::Color32::from_rgb(255, 255, 255);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(235, 239, 246);
    }

    ctx.set_style(style);
}

fn accent_color(dark_mode: bool) -> egui::Color32 {
    if dark_mode {
        egui::Color32::from_rgb(89, 107, 230)
    } else {
        egui::Color32::from_rgb(74, 91, 207)
    }
}

fn enumerate_inputs(host: &cpal::Host) -> Result<Vec<String>, String> {
    host.input_devices()
        .map_err(|error| format!("No se pudieron enumerar micrófonos: {error}"))?
        .map(|device| {
            device
                .name()
                .map_err(|error| format!("No se pudo leer el nombre del micrófono: {error}"))
        })
        .collect()
}

fn enumerate_outputs(host: &cpal::Host) -> Result<Vec<String>, String> {
    host.output_devices()
        .map_err(|error| format!("No se pudieron enumerar salidas: {error}"))?
        .map(|device| {
            device
                .name()
                .map_err(|error| format!("No se pudo leer el nombre de la salida: {error}"))
        })
        .collect()
}

fn device_combo(
    ui: &mut egui::Ui,
    id: &'static str,
    choices: &[String],
    selected: &mut Option<String>,
    default_label: &str,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected.as_deref().unwrap_or(default_label))
        .show_ui(ui, |ui| {
            ui.selectable_value(selected, None, default_label);
            for choice in choices {
                ui.selectable_value(selected, Some(choice.clone()), choice);
            }
        });
}

fn is_supported_audio(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mp3" | "wav" | "ogg" | "flac")
    )
}

fn default_music_directory() -> PathBuf {
    #[cfg(windows)]
    let profile = std::env::var_os("USERPROFILE").map(PathBuf::from);

    #[cfg(not(windows))]
    let profile = std::env::var_os("HOME").map(PathBuf::from);

    if let Some(profile) = profile {
        let music = profile.join("Music");
        if music.is_dir() {
            return music;
        }

        #[cfg(windows)]
        if let Some(one_drive) = std::env::var_os("OneDrive").map(PathBuf::from) {
            let redirected_music = one_drive.join("Music");
            if redirected_music.is_dir() {
                return redirected_music;
            }
        }

        return music;
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn is_virtual_output(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    lowered.contains("cable")
        || lowered.contains("blackhole")
        || lowered.contains("virtual")
        || (lowered.contains("pipewire") && lowered.contains("virtual"))
}

fn format_hotkey(key: Key, modifiers: Modifiers) -> Option<String> {
    let key_name = match key {
        Key::OpenBracket => "BracketLeft".to_string(),
        Key::CloseBracket => "BracketRight".to_string(),
        Key::Colon => "Semicolon".to_string(),
        Key::Pipe => "Backslash".to_string(),
        Key::Questionmark => "Slash".to_string(),
        Key::Backtick => "Backquote".to_string(),
        Key::Plus | Key::Equals => "Equal".to_string(),
        Key::Copy | Key::Cut | Key::Paste => return None,
        Key::F25
        | Key::F26
        | Key::F27
        | Key::F28
        | Key::F29
        | Key::F30
        | Key::F31
        | Key::F32
        | Key::F33
        | Key::F34
        | Key::F35 => return None,
        _ => format!("{key:?}"),
    };

    let mut parts = Vec::with_capacity(5);
    if modifiers.ctrl {
        parts.push("CTRL".to_string());
    }
    if modifiers.alt {
        parts.push("ALT".to_string());
    }
    if modifiers.shift {
        parts.push("SHIFT".to_string());
    }
    if modifiers.mac_cmd || modifiers.command {
        parts.push("SUPER".to_string());
    }
    parts.push(key_name);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::format_hotkey;
    use eframe::egui::{Key, Modifiers};

    #[test]
    fn formats_left_and_right_bracket_hotkeys() {
        assert_eq!(
            format_hotkey(Key::OpenBracket, Modifiers::CTRL),
            Some("CTRL+BracketLeft".to_string())
        );
        assert_eq!(
            format_hotkey(Key::CloseBracket, Modifiers::NONE),
            Some("BracketRight".to_string())
        );
    }
}
