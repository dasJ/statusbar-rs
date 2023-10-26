use super::{Block, I3Block, I3Event};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::sync::Mutex;

pub struct TemperatureBlock {
    /// The file where the temperature is read from
    temperature_file: Option<Mutex<File>>,
    /// A temperature the kernel considers "high"
    high_temp: Option<u32>,
}

impl TemperatureBlock {
    fn err() -> I3Block {
        I3Block {
            full_text: "ERROR".to_owned(),
            color: Some("#ff0202".to_owned()),
            ..Default::default()
        }
    }
}

impl Block for TemperatureBlock {
    fn render(&self) -> Option<I3Block> {
        if let Some(f) = &self.temperature_file {
            let mut f = f.lock().unwrap();

            if f.seek(SeekFrom::Start(0)).is_err() {
                return Some(Self::err());
            }

            let mut contents = String::new();
            if f.read_to_string(&mut contents).is_err() {
                return Some(Self::err());
            }
            let contents = contents.trim();
            let Ok(temperature) = contents.parse::<u32>() else {
                return Some(Self::err());
            };

            let color = if let Some(high) = self.high_temp {
                if temperature >= high {
                    Some("#ff0202".to_owned())
                } else {
                    None
                }
            } else {
                None
            };

            Some(I3Block {
                full_text: format!("{}°C", temperature / 1000),
                color,
                ..Default::default()
            })
        } else {
            None
        }
    }

    fn click(&self, _: &I3Event) {}
}

impl Default for TemperatureBlock {
    fn default() -> Self {
        // List all sensors
        let mut ret = Self {
            temperature_file: None,
            high_temp: None,
        };
        if let Ok(dir) = std::fs::read_dir("/sys/class/hwmon") {
            for sensor in dir.flatten() {
                let mut path = sensor.path();
                path.push("temp1_input");
                // No temperature sensor here
                if !path.as_path().exists() {
                    continue;
                }
                // Prefer coretemp on ThinkPads
                if ret.temperature_file.is_some() && sensor.file_name() != "coretemp" {
                    continue;
                }
                // Open file
                let Ok(f) = File::open(path.clone()) else {
                    continue;
                };
                ret.temperature_file = Some(Mutex::new(f));
                // Check if the kernel tells us what a high temperature is
                path.pop();
                path.push("temp1_max");
                ret.high_temp = {
                    if path.as_path().exists() {
                        if let Ok(contents) = std::fs::read_to_string(path) {
                            contents.parse::<u32>().ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
            }
        }
        ret
    }
}
