//! HID++ helpers for the battery block

use hidapi::{HidApi, HidDevice};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// How long to wait for a long message read in milliseconds
const LONG_READ_TIMEOUT: i32 = 2000;

// Hidpp constants
const REPORT_ID_HIDPP_SHORT: u8 = 0x10;
const REPORT_ID_HIDPP_LONG: u8 = 0x11;

const HIDPP_REPORT_SHORT_LENGTH: usize = 7;
const HIDPP_REPORT_LONG_LENGTH: usize = 20;

const HIDPP_PAGE_ROOT_IDX: u8 = 0x00;
const CMD_ROOT_GET_PROTOCOL_VERSION: u8 = 0x10;

const HIDPP_PAGE_UNIFIED_BATTERY: u16 = 0x1004;
const CMD_UNIFIED_BATTERY_GET_STATUS: u8 = 0x10;

const HIDPP_PAGE_ADC_MEASUREMENT: u16 = 0x1f20;
const CMD_ADC_MEASUREMENT_GET_ADC_MEASUREMENT: u8 = 0x00;

const LINUX_KERNEL_SW_ID: u8 = 0x01;
const HIDPP_MAX_PAIRED_DEVICES: u8 = 6;
const DEVICE_ID_RECEIVER: u8 = 0xff;

// HID++ 1.0
const HIDPP_GET_REGISTER: u8 = 0x81;
const HIDPP_GET_LONG_REGISTER: u8 = 0x83;
const HIDPP_REGISTER_CONNECTION_STATE: u8 = 0x02;
const HIDPP_REGISTER_PAIRING_INFORMATION: u8 = 0xb5;
const HIDPP_ERROR: u8 = 0x8f;
const HIDPP_ERROR_SUCCESS: u8 = 0x00;
const HIDPP_ERROR_INVALID_SUBID: u8 = 0x01;

/// Adapted from: <https://github.com/Sapd/HeadsetControl/blob/acd972be0468e039b93aae81221f20a54d2d60f7/src/devices/logitech_g633_g933_935.c#L44-L52>
const ADC_VOLTAGES: [u16; 100] = [
    4030, 4024, 4018, 4011, 4003, 3994, 3985, 3975, 3963, 3951, 3937, 3922, 3907, 3893, 3880, 3868,
    3857, 3846, 3837, 3828, 3820, 3812, 3805, 3798, 3791, 3785, 3779, 3773, 3768, 3762, 3757, 3752,
    3747, 3742, 3738, 3733, 3729, 3724, 3720, 3716, 3712, 3708, 3704, 3700, 3696, 3692, 3688, 3685,
    3681, 3677, 3674, 3670, 3667, 3663, 3660, 3657, 3653, 3650, 3646, 3643, 3640, 3637, 3633, 3630,
    3627, 3624, 3620, 3617, 3614, 3611, 3608, 3604, 3601, 3598, 3595, 3592, 3589, 3585, 3582, 3579,
    3576, 3573, 3569, 3566, 3563, 3560, 3556, 3553, 3550, 3546, 3543, 3539, 3536, 3532, 3529, 3525,
    3499, 3466, 3433, 3399,
];

#[derive(Debug, PartialEq)]
enum HidppVersion {
    Hidpp1,
    Hidpp2,
}

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
        *self.devices.write().unwrap() = self.inner.read().unwrap().poll_devices();
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
                    && [
                        0xc548, // bolt receiver
                        0x0aa7, // g pro
                        0x0aaa, // g pro x variant 0
                        0x0aba, // g pro x variant 1
                        0x0afb, // g pro x2 variant 0
                        0x0afc, // g pro x2 variant 1
                    ]
                    .contains(&dev.product_id())
                    && dev.interface_number() >= 2
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

    /// Pretty much what `hidpp_send_rap_command_sync()` in the kernel does but with a odd
    /// fallback
    fn send_rap_command(
        dev: &HidDevice,
        sub_id: u8,
        reg_address: u8,
        params: [u8; 4],
    ) -> Option<[u8; HIDPP_REPORT_LONG_LENGTH]> {
        let mut msg = HidppMessageShort {
            header: HidppMessageHeader {
                // RAP is always short. We write long because it fixes the PRO X Wireless headset
                long_message: false,
                device_index: sub_id,
                message_type: reg_address,
            },
            data: params,
        };
        if dev.write(&msg.to_binary()).is_err() {
            // Failed? Try again with the long variant
            // I honestly have no idea why we need this but the protocol detection fails without it
            msg.header.long_message = true;
            if dev.write(&msg.to_binary()).is_err() {
                return None;
            }
        }

        let mut buf = [0u8; HIDPP_REPORT_LONG_LENGTH];
        if dev.read_timeout(&mut buf[..], LONG_READ_TIMEOUT).is_err() {
            return None;
        }
        // Switch back to the short variant if we sent the long one in the fallback above
        buf[0] = REPORT_ID_HIDPP_SHORT;

        Some(buf)
    }

    /// Pretty much what `hidpp_root_get_protocol_version()` in the kernel does
    fn get_protocol_version(dev: &HidDevice, device_id: u8) -> Option<HidppVersion> {
        let ret = Self::send_rap_command(
            dev,
            device_id,
            HIDPP_PAGE_ROOT_IDX,
            [
                CMD_ROOT_GET_PROTOCOL_VERSION | LINUX_KERNEL_SW_ID,
                0x00,
                0x00,
                0x5a, // Ping byte
            ],
        );
        if let Some(ret) = ret {
            if ret[0] == REPORT_ID_HIDPP_SHORT
                && ret[2] == HIDPP_ERROR
                && ret[5] == HIDPP_ERROR_INVALID_SUBID
            {
                return Some(HidppVersion::Hidpp1);
            }
            if ret[0] == REPORT_ID_HIDPP_SHORT && ret[2] == HIDPP_ERROR_SUCCESS {
                let protocol_major = ret[4];
                // let protocol_minor = ret[5];
                if protocol_major > 1 {
                    return Some(HidppVersion::Hidpp2);
                }
                return Some(HidppVersion::Hidpp1);
            }
        }
        None
    }

    /// Locate a HID++2 feature index
    fn get_feature_index(dev: &HidDevice, device_id: u8, feature_id: u16) -> Option<u8> {
        let [hi, lo] = feature_id.to_be_bytes();
        let msg = HidppMessageLong {
            header: HidppMessageHeader {
                long_message: true,
                device_index: device_id,
                message_type: 0x00, // IRoot
            },
            data: [
                LINUX_KERNEL_SW_ID,
                hi,
                lo,
                0x00,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ],
        };
        if dev.write(&msg.to_binary()).is_err() {
            return None;
        }
        let mut buf = [0u8; HIDPP_REPORT_LONG_LENGTH];
        if dev.read_timeout(&mut buf[..], LONG_READ_TIMEOUT).is_err() {
            return None;
        }
        if buf[0] != REPORT_ID_HIDPP_LONG
            || buf[1] != device_id
            || buf[2] != 0x00
            || buf[3] != LINUX_KERNEL_SW_ID
        {
            return None;
        }

        let feature_index = buf[4];
        if feature_index == 0 {
            return None; // feature not found
        }
        Some(feature_index)
    }

    fn ask_device_type(
        dev: &HidDevice,
        version: &HidppVersion,
        device_id: u8,
    ) -> Option<DeviceKind> {
        match version {
            HidppVersion::Hidpp1 => {
                let sub_addr = 0x20 + (device_id - 1);
                // long register 0xB5, pairing info for this slot
                let ret = Self::send_rap_command(
                    dev,
                    DEVICE_ID_RECEIVER,
                    HIDPP_GET_LONG_REGISTER,
                    [HIDPP_REGISTER_PAIRING_INFORMATION, sub_addr, 0x00, 0x00],
                )
                .unwrap();

                if ret[0] != REPORT_ID_HIDPP_SHORT
                    || ret[1] != DEVICE_ID_RECEIVER
                    || ret[2] != HIDPP_GET_LONG_REGISTER
                    || ret[3] != HIDPP_REGISTER_PAIRING_INFORMATION
                {
                    return None;
                }
                if ret[4] != sub_addr {
                    return None;
                }

                Some(match ret[11] {
                    0x01 => DeviceKind::Keyboard,
                    0x02 => DeviceKind::Mouse,
                    0x03 => DeviceKind::Numpad,
                    0x04 => DeviceKind::Presenter,
                    0x08 => DeviceKind::Trackball,
                    0x09 => DeviceKind::Touchpad,
                    _ => DeviceKind::Unknown,
                })
            }
            HidppVersion::Hidpp2 => {
                // Feature 0x05 is "device name and type"
                let feature_index = Self::get_feature_index(dev, device_id, 0x05)?;

                // Call feature 0x0005, function 2 = GetDeviceType()
                let fcn_swid = (2 << 4) | LINUX_KERNEL_SW_ID;
                let msg = HidppMessageLong {
                    header: HidppMessageHeader {
                        long_message: true,
                        device_index: device_id,
                        message_type: feature_index,
                    },
                    data: [
                        fcn_swid, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    ],
                };
                if dev.write(&msg.to_binary()).is_err() {
                    return None;
                }
                let mut ret = [0u8; HIDPP_REPORT_LONG_LENGTH];
                if dev.read_timeout(&mut ret[..], LONG_READ_TIMEOUT).is_err() {
                    return None;
                }

                // Response repeats device index, feature index, function+swid unchanged
                if ret[0] != REPORT_ID_HIDPP_LONG
                    || ret[1] != device_id
                    || ret[2] != feature_index
                    || ret[3] != fcn_swid
                {
                    return None;
                }
                Some(match ret[4] {
                    0x00 => DeviceKind::Keyboard,
                    0x01 => DeviceKind::RemoteControl,
                    0x02 => DeviceKind::Numpad,
                    0x03 => DeviceKind::Mouse,
                    0x04 => DeviceKind::Touchpad,
                    0x05 => DeviceKind::Trackball,
                    0x06 => DeviceKind::Presenter,
                    0x07 => DeviceKind::Receiver,
                    0x08 => DeviceKind::Headset,
                    0x09 => DeviceKind::Gamepad,
                    _ => DeviceKind::Unknown,
                })
            }
        }
    }

    fn ask_battery(
        dev: &HidDevice,
        version: &HidppVersion,
        device_id: u8,
    ) -> Option<(u8, BatteryStatus)> {
        match version {
            HidppVersion::Hidpp1 => {
                unimplemented!();
            }
            HidppVersion::Hidpp2 => {
                // Unified battery
                if let Some(index) =
                    Self::get_feature_index(dev, device_id, HIDPP_PAGE_UNIFIED_BATTERY)
                {
                    let fcn_swid = CMD_UNIFIED_BATTERY_GET_STATUS | LINUX_KERNEL_SW_ID;
                    let msg = HidppMessageLong {
                        header: HidppMessageHeader {
                            long_message: true,
                            device_index: device_id,
                            message_type: index,
                        },
                        data: [
                            fcn_swid, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        ],
                    };
                    if dev.write(&msg.to_binary()).is_err() {
                        return None;
                    }

                    let mut buf = [0u8; HIDPP_REPORT_LONG_LENGTH];
                    if dev.read_timeout(&mut buf[..], LONG_READ_TIMEOUT).is_err() {
                        return None;
                    }
                    if buf[0] != REPORT_ID_HIDPP_LONG || buf[1] != device_id || buf[2] != index {
                        return None; // Invalid reply
                    }
                    let battery_status = match buf[6] {
                        0x00 => BatteryStatus::Discharging,
                        0x01 => BatteryStatus::Recharging,
                        0x02 => BatteryStatus::AlmostFull,
                        0x03 => BatteryStatus::Full,
                        0x04 => BatteryStatus::SlowRecharge,
                        0x06 => BatteryStatus::ThermalError,
                        _ => BatteryStatus::InvalidBattery,
                    };
                    return Some((buf[4], battery_status));
                }

                // We could use 0x1000 here which is battery status
                // We could use 0x1001 here which is battery voltage

                // ADC measurement
                if let Some(index) =
                    Self::get_feature_index(dev, device_id, HIDPP_PAGE_ADC_MEASUREMENT)
                {
                    let fcn_swid = CMD_ADC_MEASUREMENT_GET_ADC_MEASUREMENT | LINUX_KERNEL_SW_ID;
                    let msg = HidppMessageLong {
                        header: HidppMessageHeader {
                            long_message: true,
                            device_index: device_id,
                            message_type: index,
                        },
                        data: [
                            fcn_swid, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        ],
                    };
                    if dev.write(&msg.to_binary()).is_err() {
                        return None;
                    }

                    let mut buf = [0u8; HIDPP_REPORT_LONG_LENGTH];
                    if dev.read_timeout(&mut buf[..], LONG_READ_TIMEOUT).is_err() {
                        return None;
                    }
                    if buf[0] != REPORT_ID_HIDPP_LONG || buf[1] != device_id || buf[2] != index {
                        return None; // Invalid reply
                    }

                    let voltage_mv = u16::from_be_bytes([buf[4], buf[5]]);
                    if !(3400..5000).contains(&voltage_mv) {
                        return None;
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let percent = ADC_VOLTAGES
                        .iter()
                        .position(|&v| voltage_mv >= v)
                        .map_or(0, |i| (ADC_VOLTAGES.len() - i) as u8);
                    let state = match buf[6] {
                        0x01 => BatteryStatus::Discharging,
                        0x03 => BatteryStatus::Recharging,
                        0x07 => BatteryStatus::Full,
                        _ => BatteryStatus::InvalidBattery,
                    };
                    return Some((percent, state));
                }
            }
        }
        None
    }

    fn poll_devices(&self) -> Vec<Device> {
        let mut devices = vec![];
        for receiver in self.receivers.values() {
            // Clear buffer
            let mut buf = [0u8; 32];
            if receiver.read_timeout(&mut buf[..], 1000).is_err() {
                continue;
            }

            let is_receiver = matches!(
                Self::send_rap_command(receiver, DEVICE_ID_RECEIVER, HIDPP_GET_REGISTER, [HIDPP_REGISTER_CONNECTION_STATE, 0x00, 0x00, 0x00]),
                Some(ret)
                    if ret[0] == REPORT_ID_HIDPP_SHORT
                        && ret[1] == DEVICE_ID_RECEIVER
                        && ret[2] == HIDPP_GET_REGISTER
                        && ret[3] == HIDPP_REGISTER_CONNECTION_STATE
            );

            if is_receiver {
                for device_id in 1..=HIDPP_MAX_PAIRED_DEVICES {
                    let Some(version) = Self::get_protocol_version(receiver, device_id) else {
                        continue;
                    };
                    let Some(device_type) = Self::ask_device_type(receiver, &version, device_id)
                    else {
                        continue;
                    };
                    let Some(battery) = Self::ask_battery(receiver, &version, device_id) else {
                        continue;
                    };
                    devices.push(Device {
                        kind: device_type,
                        charge: battery.0,
                        status: battery.1,
                    });
                }
            } else {
                let Some(version) = Self::get_protocol_version(receiver, DEVICE_ID_RECEIVER) else {
                    continue;
                };
                let Some(device_type) =
                    Self::ask_device_type(receiver, &version, DEVICE_ID_RECEIVER)
                else {
                    continue;
                };
                let Some(battery) = Self::ask_battery(receiver, &version, DEVICE_ID_RECEIVER)
                else {
                    continue;
                };
                devices.push(Device {
                    kind: device_type,
                    charge: battery.0,
                    status: battery.1,
                });
            }
        }
        devices
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
            ret[0] = REPORT_ID_HIDPP_LONG;
        } else {
            // Short message is 7 bytes
            ret[0] = REPORT_ID_HIDPP_SHORT;
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
    Trackball,
    Touchpad,
    Headset,
    RemoteControl,
    Receiver,
    Gamepad,
}

impl DeviceKind {
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
            Self::RemoteControl | Self::Gamepad => "🎮",
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
    fn to_binary(&self) -> [u8; HIDPP_REPORT_SHORT_LENGTH] {
        let mut ret = [0u8; HIDPP_REPORT_SHORT_LENGTH];
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
    fn to_binary(&self) -> [u8; HIDPP_REPORT_LONG_LENGTH] {
        let mut ret = [0u8; HIDPP_REPORT_LONG_LENGTH];
        let hdr = self.header.to_binary();
        ret[..3].copy_from_slice(&hdr);
        ret[3..].copy_from_slice(&self.data);
        ret
    }
}
