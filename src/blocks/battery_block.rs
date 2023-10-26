use super::hidpp::{BatteryStatus, Hidpp};
use super::{Block, I3Block, I3Event};
use std::sync::RwLock;
use std::time::Instant;

pub struct BatteryBlock {
    hidpp: Option<Hidpp>,
    last_hidpp_recv_poll: RwLock<Instant>,
    last_hidpp_dev_poll: RwLock<Instant>,
}

impl Block for BatteryBlock {
    #[allow(clippy::too_many_lines)]
    fn render(&self) -> Option<I3Block> {
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
                        let mut path = supply.path();
                        path.push("capacity");
                        if let Ok(contents) = std::fs::read_to_string(path) {
                            let contents = contents.trim();
                            if let Ok(percent) = contents.parse::<u8>() {
                                batteries.push(percent);
                            }
                        } else {
                            continue;
                        }
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

                // Calculate the resulting string
                batteries
                    .iter()
                    .map(|bat| {
                        if charging {
                            format!("🔋<span foreground='#02ff02'>{bat}%</span>")
                        } else if bat <= &15u8 {
                            format!("🪫<span foreground='#ff0202'>{bat}%</span>")
                        } else {
                            format!("🔋{bat}%")
                        }
                    })
                    .collect::<String>()
            } else {
                String::new()
            }
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
                        "{}<span foreground='#ff0202'>{}%</span>",
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
            devices.iter().map(Clone::clone).collect::<String>()
        } else {
            String::new()
        };

        if power_batteries.is_empty() && hidpp.is_empty() {
            return None;
        }
        Some(I3Block {
            full_text: format!("{power_batteries}{hidpp}"),
            markup: Some(super::Markup::Pango),
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}

impl Default for BatteryBlock {
    fn default() -> Self {
        Self {
            hidpp: Hidpp::new(),
            last_hidpp_recv_poll: RwLock::new(Instant::now()),
            last_hidpp_dev_poll: RwLock::new(Instant::now()),
        }
    }
}
