//! Lists sound devices. With PipeWire or PulseAudio, "default" follows the system's chosen device.

use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    /// cpal's stable id, saved in the settings.
    pub id: String,
    pub name: String,
}

pub fn inputs() -> Vec<DeviceInfo> {
    list(true)
}

pub fn outputs() -> Vec<DeviceInfo> {
    list(false)
}

fn list(input: bool) -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let devices = if input { host.input_devices() } else { host.output_devices() };
    let Ok(devices) = devices else { return Vec::new() };
    devices
        .filter_map(|d| {
            let id = d.id().ok()?.to_string();
            let name = d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| id.clone());
            Some(DeviceInfo { id, name })
        })
        .collect()
}

/// The device with `id`, or the system default when `id` is None or no longer present.
pub fn find(id: Option<&str>, input: bool) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(id) = id {
        if let Ok(parsed) = id.parse::<cpal::DeviceId>() {
            if let Some(d) = host.device_by_id(&parsed) {
                return Some(d);
            }
        }
    }
    if input { host.default_input_device() } else { host.default_output_device() }
}
