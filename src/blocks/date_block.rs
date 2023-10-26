use super::{Block, I3Block, I3Event};
use chrono::Local;

#[derive(Default)]
pub struct DateBlock {}

impl Block for DateBlock {
    fn render(&self) -> Option<I3Block> {
        let now = Local::now();
        Some(I3Block {
            full_text: now.format("(KW%V) %d.%m. (%b) %H:%M").to_string(),
            short_text: Some(now.format("%H:%M").to_string()),
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}
