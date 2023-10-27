use super::{Block, I3Block, I3Event};
use std::fs::File;
use std::io::{BufRead as _, BufReader};

#[derive(Default)]
pub struct DefaultRouteBlock {}

impl Block for DefaultRouteBlock {
    fn render(&self) -> Option<I3Block> {
        let reader = BufReader::new(File::open("/proc/net/route").ok()?).lines();
        for line in reader.flatten() {
            let mut split = line.split('\t');
            let Some(interface) = split.next() else {
                continue;
            };
            // Detect default route
            if split.next() == Some("00000000") {
                return Some(I3Block {
                    full_text: interface.to_owned(),
                    ..Default::default()
                });
            }
        }
        Some(I3Block {
            full_text: "No link".to_owned(),
            color: Some("#ff0202".to_owned()),
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}
