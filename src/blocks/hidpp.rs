//! HID++ helpers for the battery block

use hidapi::{HidApi, HidDevice};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// This asks the device for its battery status (only Bolt devices).
/// I have no idea how this is calculated, I got it by running solaar in
/// debug mode.
const ASK_FOR_BATTERY: [u8; 17] = [
    19u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8, 00u8,
    00u8,
];

/// How long to wait for a short message read in milliseconds
const SHORT_READ_TIMEOUT: i32 = 1000;

/// How long to wait for a long message read in milliseconds
const LONG_READ_TIMEOUT: i32 = 2000;

pub struct Hidpp {
    inner: Arc<RwLock<HidppInner>>,
    devices: Arc<RwLock<Vec<Device>>>,
}

struct HidppInner {
    hid_api: HidApi,
    receivers: HashMap<String, HidDevice>,
}

impl Hidpp {
    pub fn new() -> Option<Self> {
        let Ok(hid_api) = HidApi::new() else {
            return None;
        };

        let ret = Self {
            inner: Arc::new(RwLock::new(HidppInner {
                hid_api,
                receivers: HashMap::new(),
            })),
            devices: Arc::new(RwLock::new(vec![])),
        };
        let ret2 = ret.clone();
        std::thread::spawn(move || ret2.enumerate_receivers(true));
        Some(ret)
    }

    pub fn enumerate_receivers(&self, also_poll_devices: bool) {
        self.inner.write().unwrap().enumerate_receivers();
        if also_poll_devices {
            self.poll_devices();
        }
    }

    pub fn poll_devices(&self) {
        if let Some(new_devices) = self.inner.read().unwrap().poll_devices() {
            *self.devices.write().unwrap() = new_devices;
        }
    }

    pub fn devices(&self) -> Vec<Device> {
        self.devices.read().unwrap().clone()
    }
}

impl Clone for Hidpp {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            devices: Arc::clone(&self.devices),
        }
    }
}

unsafe impl Send for HidppInner {}
unsafe impl Sync for HidppInner {}

impl HidppInner {
    /// Finds all relevant devices and dedup them
    fn enumerate_receivers(&mut self) {
        self.receivers = self
            .hid_api
            .device_list()
            .filter(|dev| {
                dev.vendor_id() == 0x046d
                    && dev.product_id() == 0xc548
                    && dev.interface_number() == 2
            })
            .filter_map(|dev| {
                if let Ok(d) = dev.open_device(&self.hid_api) {
                    Some((dev.path().to_str().unwrap_or("").to_owned(), d))
                } else {
                    None
                }
            })
            .collect::<HashMap<String, HidDevice>>();
    }

    fn poll_devices(&self) -> Option<Vec<Device>> {
        let mut devices = vec![];
        for receiver in self.receivers.values() {
            // Clear buffer
            let mut buf = [0u8; 32];
            if receiver.read_timeout(&mut buf[..], 1000).is_err() {
                return None;
            }

            // Count connected devices
            let msg = HidppMessageShort {
                header: HidppMessageHeader {
                    long_message: false,
                    device_index: 0xff,
                    message_type: 0x81,
                },
                data: 0x0200_0000_u32.to_be_bytes(),
            };
            if receiver.write(&msg.to_binary()).is_err() {
                return None;
            }

            let mut buf = [0u8; 7];
            if receiver
                .read_timeout(&mut buf[..], SHORT_READ_TIMEOUT)
                .is_err()
            {
                return None;
            }
            if buf[0] != 0x10 || buf[1] != 0xff || buf[2] != 0x81 {
                return None;
            }
            let num_connected = buf[5];

            // Iterate all connected devices
            let mut found = 0;
            for device_id in 1..8 {
                // Bolt receiver supports 8 devices
                // Ask receiver for device identity
                let msg = HidppMessageShort {
                    header: HidppMessageHeader {
                        long_message: false,
                        device_index: 0xff,
                        message_type: 0x83,
                    },
                    // 0x50 is bolt-specific, unified uses another offset.
                    // but parsing unifying also means we will find the kind at another location
                    // in the output :/
                    data: [0xb5, device_id + 0x50, 0x00, 0x00],
                };
                if receiver.write(&msg.to_binary()).is_err() {
                    continue;
                }

                let mut buf = [0u8; 20];
                if receiver
                    .read_timeout(&mut buf[..], LONG_READ_TIMEOUT)
                    .is_err()
                {
                    continue;
                }
                if buf[0] != 0x11 || buf[1] != 0xff || buf[2] != 0x83 {
                    continue; // Invalid reply
                }
                let device_type = buf[5];

                // Ask for battery
                let msg = HidppMessageLong {
                    header: HidppMessageHeader {
                        long_message: true,
                        device_index: device_id,
                        message_type: 0x08,
                    },
                    data: ASK_FOR_BATTERY,
                };
                if receiver.write(&msg.to_binary()).is_err() {
                    continue;
                }

                let mut buf = [0u8; 20];
                if receiver
                    .read_timeout(&mut buf[..], LONG_READ_TIMEOUT)
                    .is_err()
                {
                    continue;
                }
                if buf[0] != 0x11 || buf[1] != device_id || buf[2] != 0x08 {
                    continue; // Invalid reply
                }

                devices.push(Device {
                    kind: DeviceKind::from(device_type),
                    charge: buf[3 + 1],
                    status: BatteryStatus::from(buf[3 + 3]),
                });

                found += 1;
                if found == num_connected {
                    break;
                }
            }
        }
        Some(devices)
    }
}

/// A header of a HID++ message
struct HidppMessageHeader {
    long_message: bool,
    device_index: u8,
    message_type: u8,
}

impl HidppMessageHeader {
    /// Converts the header to the bytes that will be written to hidapi
    fn to_binary(&self) -> [u8; 3] {
        let mut ret = [0u8; 3];
        // Report ID
        if self.long_message {
            // Long message is 20 bytes
            ret[0] = 0x11;
        } else {
            // Short message is 7 bytes
            ret[0] = 0x10;
        }
        // Device index
        ret[1] = self.device_index;
        // Sub ID
        ret[2] = self.message_type;
        ret
    }
}

/// The kind of a HID++ device
#[derive(Debug, Clone)]
pub enum DeviceKind {
    Unknown,
    Keyboard,
    Mouse,
    Numpad,
    Presenter,
    Remote,
    Trackball,
    Touchpad,
    Headset,
    RemoteControl,
    Receiver,
}

impl DeviceKind {
    fn from(v: u8) -> Self {
        match v {
            0x01 => Self::Keyboard,
            0x02 => Self::Mouse,
            0x03 => Self::Numpad,
            0x04 => Self::Presenter,
            0x07 => Self::Remote,
            0x08 => Self::Trackball,
            0x09 => Self::Touchpad,
            0x0d => Self::Headset,
            0x0e => Self::RemoteControl,
            0x0f => Self::Receiver,
            _ => Self::Unknown,
        }
    }

    pub fn emoji(&self) -> &str {
        match self {
            Self::Unknown => "❓",
            Self::Keyboard => "⌨️'",
            Self::Mouse => "🖱️",
            Self::Numpad => "🎹",
            Self::Presenter => "📽️",
            Self::Trackball => "🖲",
            Self::Touchpad => "◻",
            Self::Headset => "🎧",
            Self::Remote | Self::RemoteControl => "🎮",
            Self::Receiver => "📻",
        }
    }
}

/// The status of a unified battery
#[derive(Debug, Clone)]
pub enum BatteryStatus {
    Discharging,
    Recharging,
    AlmostFull,
    Full,
    SlowRecharge,
    InvalidBattery,
    ThermalError,
}

impl BatteryStatus {
    fn from(v: u8) -> Self {
        match v {
            0x00 => Self::Discharging,
            0x01 => Self::Recharging,
            0x02 => Self::AlmostFull,
            0x03 => Self::Full,
            0x04 => Self::SlowRecharge,
            0x06 => Self::ThermalError,
            _ => Self::InvalidBattery,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Device {
    pub kind: DeviceKind,
    pub charge: u8,
    pub status: BatteryStatus,
}

/// A short HID++ message
struct HidppMessageShort {
    /// The header of the message
    header: HidppMessageHeader,
    /// The payload of the message
    data: [u8; 4],
}

impl HidppMessageShort {
    /// Converts the message to the bytes that will be written to hidapi
    fn to_binary(&self) -> [u8; 7] {
        let mut ret = [0u8; 7];
        let hdr = self.header.to_binary();
        ret[..3].copy_from_slice(&hdr);
        ret[3..].copy_from_slice(&self.data);
        ret
    }
}

/// A long HID++ message
struct HidppMessageLong {
    /// The header of the message
    header: HidppMessageHeader,
    /// The payload of the message
    data: [u8; 17],
}

impl HidppMessageLong {
    /// Converts the message to the bytes that will be written to hidapi
    fn to_binary(&self) -> [u8; 20] {
        let mut ret = [0u8; 20];
        let hdr = self.header.to_binary();
        ret[..3].copy_from_slice(&hdr);
        ret[3..].copy_from_slice(&self.data);
        ret
    }
}
