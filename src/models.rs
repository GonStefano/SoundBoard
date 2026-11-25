use serde::{Deserialize, Serialize};

pub type SoundId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundItem {
    pub id: SoundId,
    pub name: String,
    pub file_path: String,
    pub hotkey: Option<String>,
    pub volume: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioDeviceSelection {
    pub input: Option<String>,
    pub headphones: Option<String>,
    pub virtual_microphone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub sounds: Vec<SoundItem>,
    pub master_volume: f32,
    pub devices: AudioDeviceSelection,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sounds: Vec::new(),
            master_volume: 1.0,
            devices: AudioDeviceSelection::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EngineCommand {
    PlaySound { sound_id: SoundId },
    StopAll,
    SetMasterVolume { volume: f32 },
    SetDevices(AudioDeviceSelection),
    ReloadSounds { sounds: Vec<SoundItem> },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    VirtualCableUnavailable,
    InputDeviceUnavailable,
    OutputDeviceUnavailable,
    Error(String),
}
