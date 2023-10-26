use super::{Block, I3Block, I3Event};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::sync::Mutex;

pub struct LoadBlock {
    /// The file where the load is read from
    load_file: Option<Mutex<File>>,
    /// Number of parallel threads
    num_threads: Option<usize>,
}

impl LoadBlock {
    fn err() -> I3Block {
        I3Block {
            full_text: "ERROR".to_owned(),
            color: Some("#ff0202".to_owned()),
            ..Default::default()
        }
    }
}

impl Block for LoadBlock {
    fn render(&self) -> Option<I3Block> {
        if let Some(f) = &self.load_file {
            let mut f = f.lock().unwrap();

            if f.seek(SeekFrom::Start(0)).is_err() {
                return Some(Self::err());
            }

            let mut contents = String::new();
            if f.read_to_string(&mut contents).is_err() {
                return Some(Self::err());
            }

            let Some(load1) = contents.split(' ').next() else {
                return Some(Self::err());
            };
            let Ok(load1) = load1.parse::<f32>() else {
                return Some(Self::err());
            };

            let color = if let Some(num_threads) = self.num_threads {
                #[allow(clippy::cast_precision_loss)] // Who cares
                if load1 / num_threads as f32 > 1.0 {
                    Some("#ff0202".to_owned())
                } else {
                    None
                }
            } else {
                None
            };

            Some(I3Block {
                full_text: format!("{load1:.02}"),
                color,
                ..Default::default()
            })
        } else {
            None
        }
    }

    fn click(&self, _: &I3Event) {}
}

impl Default for LoadBlock {
    fn default() -> Self {
        Self {
            load_file: File::open("/proc/loadavg").ok().map(Mutex::new),
            num_threads: std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .ok(),
        }
    }
}
