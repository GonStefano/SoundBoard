use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, Host};

#[derive(Debug, Default)]
pub struct DeviceInventory {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

impl DeviceInventory {
    pub fn refresh(host: &Host) -> Self {
        let mut inventory = Self::default();

        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if let Ok(name) = device.name() {
                    inventory.inputs.push(name);
                }
            }
        }

        if let Ok(devices) = host.output_devices() {
            for device in devices {
                if let Ok(name) = device.name() {
                    inventory.outputs.push(name);
                }
            }
        }

        inventory
    }

    pub fn virtual_cable_detected(&self) -> bool {
        self.outputs.iter().any(|name| {
            let lowered = name.to_ascii_lowercase();
            lowered.contains("cable")
                || lowered.contains("blackhole")
                || lowered.contains("virtual")
                || (lowered.contains("pipewire") && lowered.contains("virtual"))
        })
    }
}

pub fn find_output_device(host: &Host, name: Option<&str>) -> Option<Device> {
    if let Some(requested) = name {
        if let Ok(devices) = host.output_devices() {
            for device in devices {
                if let Ok(device_name) = device.name() {
                    if device_name == requested {
                        return Some(device);
                    }
                }
            }
        }
        return None;
    }

    host.default_output_device()
}

pub fn find_input_device(host: &Host, name: Option<&str>) -> Option<Device> {
    if let Some(requested) = name {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if let Ok(device_name) = device.name() {
                    if device_name == requested {
                        return Some(device);
                    }
                }
            }
        }
        return None;
    }

    host.default_input_device()
}
