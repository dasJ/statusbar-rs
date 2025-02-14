use super::{Block, I3Block, I3Event};

#[derive(Default)]
pub struct DiskBlock {}

impl Block for DiskBlock {
    fn render(&self) -> Option<I3Block> {
        let Ok(stat) = nix::sys::statvfs::statvfs("/") else {
            return None;
        };

        let total_bytes = stat.blocks() * stat.block_size();
        let free_bytes = stat.blocks_available() * stat.block_size();

        // warn if less than 10%
        let color = if free_bytes < total_bytes / 10 {
            Some("#ff0202".to_owned())
        } else {
            None
        };

        let free_gb = free_bytes as f64 / 1024.0 / 1024.0 / 1024.0;

        let full_text = if free_bytes > 1024 * 1024 * 1024 {
            format!("{:.2} GB", free_gb)
        } else {
            format!("{} MB", free_bytes / 1024 / 1024)
        };

        Some(I3Block {
            full_text,
            short_text: Some(format!("{:.0} GB", free_gb).to_string()),
            color,
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}
