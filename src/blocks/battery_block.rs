use super::bluetooth_battery;
use super::hidpp::{BatteryStatus, Hidpp};
use super::{Block, I3Block, I3Event};
use std::sync::{mpsc::Sender, RwLock};
use std::time::Instant;

pub struct BatteryBlock {
    bluetooth: Option<bluetooth_battery::BluetoothBattery>,
    hidpp: Option<Hidpp>,
    last_bluetooth_poll: RwLock<Instant>,
    last_hidpp_recv_poll: RwLock<Instant>,
    last_hidpp_dev_poll: RwLock<Instant>,
}

impl Block for BatteryBlock {
    #[allow(clippy::too_many_lines)]
    fn render(&self) -> Option<I3Block> {
        struct Battery {
            percent_charged: u8,
            watts_charging: f64,
        }
        // Find power supply batteries
        let power_batteries = {
            if let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") {
                let mut batteries = vec![];

                let mut charging = false;

                for supply in dir.flatten() {
                    if supply
                        .file_name()
                        .into_string()
                        .map(|x| x.starts_with("BAT"))
                        .unwrap_or(false)
                    {
                        let mut percent_charged = 0;

                        let mut path = supply.path();
                        path.push("capacity");
                        if let Ok(contents) = std::fs::read_to_string(path) {
                            let contents = contents.trim();
                            if let Ok(percent) = contents.parse::<u8>() {
                                percent_charged = percent;
                            }
                        }
                        // If somebody refactors this in the future: current_now is on Dell,
                        // power_now on Lenovo
                        let power = std::fs::read_to_string(supply.path().join("power_now"))
                            .ok()
                            .and_then(|v| v.trim().parse::<f64>().ok());

                        let watts = power.map_or({
                            let current =
                                std::fs::read_to_string(supply.path().join("current_now"))
                                    .ok()
                                    .and_then(|v| v.trim().parse::<f64>().ok());

                            let voltage =
                                std::fs::read_to_string(supply.path().join("voltage_now"))
                                    .ok()
                                    .and_then(|v| v.trim().parse::<f64>().ok());

                            if let (Some(current), Some(voltage)) = (current, voltage) {
                                (current * voltage) / 1_000_000_000_000.0
                            } else {
                                0.0
                            }
                        }, |x| x / 1_000_000.0);

                        batteries.push(Battery {
                            percent_charged,
                            watts_charging: watts,
                        });
                    } else if supply
                        .file_name()
                        .into_string()
                        .map(|x| x.starts_with("AC"))
                        .unwrap_or(false)
                    {
                        let mut path = supply.path();
                        path.push("online");
                        if let Ok(contents) = std::fs::read_to_string(path) {
                            let contents = contents.trim();
                            if contents == "1" {
                                charging = true;
                            }
                        } else {
                            continue;
                        }
                    }
                }

                let ret = batteries
                    .iter()
                    .map(|bat| {
                        if charging {
                            format!(
                                " 🔋<span foreground='#02ff02'>{}% {:.2}W+</span>",
                                bat.percent_charged, bat.watts_charging
                            )
                        } else if bat.percent_charged <= 15u8 {
                            format!(
                                " 🪫<span foreground='#ff0202'>{}% {:.2}W-</span>",
                                bat.percent_charged, bat.watts_charging
                            )
                        } else {
                            format!(" 🔋{}% {:.2}W-", bat.percent_charged, bat.watts_charging)
                        }
                    })
                    .collect::<String>();
                ret.trim().to_owned()
            } else {
                String::new()
            }
        };

        // Render bluetooth devices
        let bluetooth = if let Some(bluetooth) = &self.bluetooth {
            let mut devices = vec![];
            for (icon, percentage) in bluetooth.percentages() {
                let emoji = match icon.as_deref() {
                    Some("phone") => "📱",
                    Some("computer") => "💻",
                    Some("video-display") => "📼",
                    Some("multimedia-player") => "⏯",
                    Some("scanner" | "printer") => "🖨️",
                    Some("input-keyboard") => "⌨️",
                    Some("input-mouse") => "🖱️",
                    Some("input-gaming") => "🎮",
                    Some("input-tablet") => "✍️",
                    Some("modem" | "network-wireless") => "🛜",
                    Some("audio-headset" | "audio-headphones") => "🎧",
                    Some("camera-video") => "📹",
                    Some("audio-card") => "🎵",
                    Some("camera-photo") => "📷",
                    _ => "",
                };
                devices.push(format!("{emoji}{percentage}%"));
            }

            // Poll devices every 2 minutes
            if self.last_bluetooth_poll.read().unwrap().elapsed().as_secs() > 120 {
                bluetooth.update();
                *self.last_bluetooth_poll.write().unwrap() = Instant::now();
            }

            let ret = devices
                .iter()
                .map(Clone::clone)
                .collect::<Vec<String>>()
                .join("  ");
            if ret.is_empty() {
                ret
            } else {
                format!("  {ret}")
            }
        } else {
            String::new()
        };

        // Find HID++ devices
        let hidpp = if let Some(hidpp_devices) = &self.hidpp {
            let mut devices = vec![];
            for dev in hidpp_devices.devices() {
                match dev.status {
                    BatteryStatus::Discharging | BatteryStatus::Full => {
                        if dev.charge <= 20 {
                            devices.push(format!(
                                "{}<span foreground='#ff0202'>{}%</span>",
                                dev.kind.emoji(),
                                dev.charge
                            ));
                        } else {
                            devices.push(format!("{}{}%", dev.kind.emoji(), dev.charge));
                        }
                    }
                    BatteryStatus::Recharging
                    | BatteryStatus::AlmostFull
                    | BatteryStatus::SlowRecharge => devices.push(format!(
                        "{}<span foreground='#02ff02'>{}%</span>",
                        dev.kind.emoji(),
                        dev.charge
                    )),
                    BatteryStatus::InvalidBattery | BatteryStatus::ThermalError => {
                        devices.push(format!(
                            "{}<span foreground='#ff0202'>(!) {}%</span>",
                            dev.kind.emoji(),
                            dev.charge
                        ));
                    }
                }
            }
            // Poll receivers every 15 minutes
            if self
                .last_hidpp_recv_poll
                .read()
                .unwrap()
                .elapsed()
                .as_secs()
                > 900
            {
                let hidpp = hidpp_devices.clone();
                std::thread::spawn(move || hidpp.enumerate_receivers(false));
                *self.last_hidpp_recv_poll.write().unwrap() = Instant::now();
            }
            // Poll devices every 2 minutes
            if self.last_hidpp_dev_poll.read().unwrap().elapsed().as_secs() > 120 {
                let hidpp = hidpp_devices.clone();
                std::thread::spawn(move || hidpp.poll_devices());
                *self.last_hidpp_dev_poll.write().unwrap() = Instant::now();
            }
            let ret = devices
                .iter()
                .map(Clone::clone)
                .collect::<Vec<String>>()
                .join("  ");
            if ret.is_empty() || (power_batteries.is_empty() && bluetooth.is_empty()) {
                ret
            } else {
                format!("  {ret}")
            }
        } else {
            String::new()
        };

        if power_batteries.is_empty() && bluetooth.is_empty() && hidpp.is_empty() {
            return None;
        }
        Some(I3Block {
            full_text: format!("{power_batteries}{bluetooth}{hidpp}"),
            markup: Some(super::Markup::Pango),
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}

impl BatteryBlock {
    pub fn new(timer_cancel: &Sender<()>) -> Self {
        Self {
            hidpp: Hidpp::new(),
            bluetooth: bluetooth_battery::BluetoothBattery::new(timer_cancel),
            last_bluetooth_poll: RwLock::new(Instant::now()),
            last_hidpp_recv_poll: RwLock::new(Instant::now()),
            last_hidpp_dev_poll: RwLock::new(Instant::now()),
        }
    }
}
