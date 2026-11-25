use std::fs;
use std::path::{Path, PathBuf};

use crate::models::AudioConfig;

pub fn default_config_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
            return app_data.join("Soundboard").join("config.json");
        }
        if let Some(profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            return profile
                .join("AppData")
                .join("Roaming")
                .join("Soundboard")
                .join("config.json");
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            return config_home.join("soundboard").join("config.json");
        }
    }

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        #[cfg(windows)]
        return home
            .join("AppData")
            .join("Roaming")
            .join("Soundboard")
            .join("config.json");
        #[cfg(not(windows))]
        return home.join(".config").join("soundboard").join("config.json");
    }
    PathBuf::from("config.json")
}

pub fn load_config(path: Option<&Path>) -> AudioConfig {
    let path = path.map_or_else(default_config_path, Path::to_path_buf);

    let local_fallback = PathBuf::from("config.json");
    let contents = fs::read_to_string(&path).or_else(|_| {
        if path != local_fallback {
            fs::read_to_string(&local_fallback)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "configuration file not found",
            ))
        }
    });

    match contents {
        Ok(json) => match serde_json::from_str::<AudioConfig>(&json) {
            Ok(config) => config,
            Err(_) => AudioConfig::default(),
        },
        Err(_) => AudioConfig::default(),
    }
}

pub fn save_config(path: Option<&Path>, config: &AudioConfig) -> Result<PathBuf, String> {
    let path = path.map_or_else(default_config_path, Path::to_path_buf);
    let json = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
    let write_primary = (|| {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &json)
    })();

    if write_primary.is_ok() {
        return Ok(path);
    }

    let fallback = PathBuf::from("config.json");
    fs::write(&fallback, json).map_err(|error| error.to_string())?;
    Ok(fallback)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::models::{AudioConfig, AudioDeviceSelection, SoundItem};

    use super::{load_config, save_config};

    #[test]
    fn saves_and_reloads_sounds_and_device_selection() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "soundboard-config-test-{}-{unique}.json",
            std::process::id()
        ));
        let config = AudioConfig {
            sounds: vec![SoundItem {
                id: 7,
                name: "Test tone".to_string(),
                file_path: "C:\\Sounds\\tone.wav".to_string(),
                hotkey: Some("CTRL+BracketLeft".to_string()),
                volume: 0.65,
            }],
            master_volume: 0.8,
            devices: AudioDeviceSelection {
                input: Some("Mic".to_string()),
                headphones: Some("Headphones".to_string()),
                virtual_microphone: Some("Cable Input".to_string()),
            },
        };

        save_config(Some(&path), &config).expect("temporary config should be writable");
        let loaded = load_config(Some(&path));
        assert_eq!(loaded.sounds.len(), 1);
        assert_eq!(loaded.sounds[0].hotkey.as_deref(), Some("CTRL+BracketLeft"));
        assert_eq!(
            loaded.devices.virtual_microphone.as_deref(),
            Some("Cable Input")
        );

        std::fs::remove_file(path).expect("temporary config should be removable");
    }

    #[test]
    fn missing_config_returns_default_without_panicking() {
        let path = PathBuf::from("a-config-file-that-does-not-exist.json");
        let config = load_config(Some(&path));
        assert!(config.sounds.is_empty());
        assert_eq!(config.master_volume, 1.0);
    }
}
