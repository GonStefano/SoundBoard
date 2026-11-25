use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Sender;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::models::{EngineCommand, SoundItem};

pub struct GlobalHotkeyController {
    manager: GlobalHotKeyManager,
    paused: Arc<AtomicBool>,
    registered: HashMap<u64, HotKey>,
    updates: Sender<(u32, Option<u64>)>,
}

impl GlobalHotkeyController {
    pub fn new(sound_items: &[SoundItem], commands: Sender<EngineCommand>) -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new().map_err(|error| format!("{error:?}"))?;
        let paused = Arc::new(AtomicBool::new(false));
        let mut registered = HashMap::new();

        for item in sound_items {
            if let Some(shortcut) = item.hotkey.as_deref() {
                if let Some(hotkey) = parse_hotkey(shortcut) {
                    if manager.register(hotkey).is_ok() {
                        registered.insert(item.id, hotkey);
                    }
                }
            }
        }

        let initial_targets: HashMap<u32, u64> = registered
            .iter()
            .map(|(sound_id, hotkey)| (hotkey.id(), *sound_id))
            .collect();
        let (updates, update_receiver) = crossbeam_channel::unbounded();
        let paused_thread = paused.clone();
        thread::spawn(move || {
            let event_receiver = GlobalHotKeyEvent::receiver();
            let mut targets = initial_targets;
            loop {
                crossbeam_channel::select! {
                    recv(event_receiver) -> event => {
                        let Ok(event) = event else { break };
                        if event.state != HotKeyState::Pressed || paused_thread.load(Ordering::Acquire) {
                            continue;
                        }
                        if let Some(sound_id) = targets.get(&event.id) {
                            let _ = commands.try_send(EngineCommand::PlaySound { sound_id: *sound_id });
                        }
                    }
                    recv(update_receiver) -> update => {
                        let Ok((hotkey_id, sound_id)) = update else { break };
                        if let Some(sound_id) = sound_id {
                            targets.insert(hotkey_id, sound_id);
                        } else {
                            targets.remove(&hotkey_id);
                        }
                    }
                }
            }
        });

        Ok(Self {
            manager,
            paused,
            registered,
            updates,
        })
    }

    pub fn pause_handle(&self) -> Arc<AtomicBool> {
        self.paused.clone()
    }

    pub fn register_dynamic(&mut self, item: &SoundItem) -> bool {
        let Some(shortcut) = item.hotkey.as_deref() else {
            return false;
        };
        let Some(hotkey) = parse_hotkey(shortcut) else {
            return false;
        };

        let _ = self.unregister(item.id);
        if self.manager.register(hotkey).is_ok() {
            self.registered.insert(item.id, hotkey);
            let _ = self.updates.try_send((hotkey.id(), Some(item.id)));
            true
        } else {
            false
        }
    }

    pub fn unregister(&mut self, sound_id: u64) -> bool {
        if let Some(hotkey) = self.registered.remove(&sound_id) {
            let unregister_result = self.manager.unregister(hotkey).is_ok();
            let _ = self.updates.try_send((hotkey.id(), None));
            unregister_result
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        for hotkey in self.registered.values() {
            let _ = self.manager.unregister(*hotkey);
            let _ = self.updates.try_send((hotkey.id(), None));
        }
        self.registered.clear();
    }
}

fn parse_hotkey(shortcut: &str) -> Option<HotKey> {
    shortcut.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_hotkey;
    use global_hotkey::hotkey::Code;

    #[test]
    fn parses_bracket_hotkeys_and_modifiers() {
        let hotkey = parse_hotkey("CTRL+BracketLeft");
        assert!(hotkey.is_some_and(|hotkey| hotkey.key == Code::BracketLeft));
    }

    #[test]
    fn parses_unmodified_punctuation_hotkeys() {
        let hotkey = parse_hotkey("BracketRight");
        assert!(hotkey.is_some_and(|hotkey| hotkey.key == Code::BracketRight));
    }
}
