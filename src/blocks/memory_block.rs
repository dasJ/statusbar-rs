use std::{
    fs::File,
    io::{self, BufRead},
};

use super::{Block, I3Block, I3Event};

#[derive(Default)]
pub struct MemoryBlock {}

impl Block for MemoryBlock {
    fn render(&self) -> Option<I3Block> {
        let mut total_mem_kb: u64 = 0;
        let mut available_mem_kb: u64 = 0;

        // Read meminfo
        let Ok(meminfo) = File::open("/proc/meminfo") else {
            return None;
        };

        for line in io::BufReader::new(meminfo).lines() {
            let Ok(line) = line else {
                continue;
            };
            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or_default();

            match key {
                "MemTotal:" => {
                    match value.parse::<u64>() {
                        Ok(val) => total_mem_kb = val,
                        Err(_) => {}
                    };
                }
                "MemAvailable:" => {
                    match value.parse::<u64>() {
                        Ok(val) => available_mem_kb = val,
                        Err(_) => {}
                    };
                }
                _ => {}
            };
        }
        let available_mem_gb: f64 = available_mem_kb as f64 / 1024.0 / 1024.0;

        // warn if less than 10%
        let color = if available_mem_kb < total_mem_kb / 10 {
            Some("#ff0202".to_owned())
        } else {
            None
        };

        let full_text = if available_mem_kb > 1024 * 1024 {
            format!("{:.2} GB", available_mem_gb)
        } else {
            format!("{} MB", available_mem_kb / 1024)
        };

        Some(I3Block {
            full_text,
            short_text: Some(format!("{:.2} GB", available_mem_gb).to_string()),
            color,
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}
