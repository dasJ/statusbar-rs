use super::{Block, I3Block, I3Event};
use std::io::{BufRead as _, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub struct SocketBlock {
    connected: Arc<AtomicBool>,
    content: Arc<RwLock<I3Block>>,
    stream: Arc<RwLock<Option<UnixStream>>>,
}

impl SocketBlock {
    /// # Panics
    /// Can panic when `$XDG_RUNTIME_DIR` is not set
    #[must_use]
    pub fn new(socket_path: String, timer_cancel: Sender<()>) -> Self {
        let socket_path = if socket_path.starts_with('/') {
            socket_path
        } else {
            let runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR not set");
            let mut runtime_dir = Path::new(&runtime_dir).to_path_buf();
            runtime_dir.push("statusbar");
            if !runtime_dir.exists() {
                let _ = std::fs::create_dir_all(&runtime_dir);
            }
            runtime_dir.push(&socket_path);
            runtime_dir.to_str().unwrap().to_owned()
        };

        let connected = Arc::new(AtomicBool::new(false));
        let content = Arc::new(RwLock::new(I3Block::default()));
        let stream2 = Arc::new(RwLock::new(None));

        let content2 = content.clone();
        let connected2 = connected.clone();
        let stream3 = stream2.clone();
        std::thread::spawn(move || loop {
            let Ok(stream) = UnixStream::connect(&socket_path) else {
                eprintln!("Failed to connect to socket at {socket_path}");
                connected2.swap(false, Ordering::Relaxed);
                *stream3.write().unwrap() = None;
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            *stream3.write().unwrap() = Some(stream.try_clone().unwrap());
            let mut reader = BufReader::new(stream);
            connected2.swap(true, Ordering::Relaxed);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() {
                    if let Ok(content) = serde_json::from_str::<I3Block>(&line) {
                        *(content2.write().unwrap()) = content;
                        let _idc = timer_cancel.send(());
                    } else {
                        eprintln!("Invalid block received from socket");
                        connected2.swap(false, Ordering::Relaxed);
                        *stream3.write().unwrap() = None;
                        std::thread::sleep(Duration::from_secs(2));
                        break;
                    }
                } else {
                    eprintln!("Failed to read message from socket");
                    connected2.swap(false, Ordering::Relaxed);
                    *stream3.write().unwrap() = None;
                    std::thread::sleep(Duration::from_secs(2));
                    break;
                }
            }
        });

        Self { connected, content, stream: stream2 }
    }
}

impl Block for SocketBlock {
    fn render(&self) -> Option<I3Block> {
        if !self.connected.load(Ordering::Relaxed) {
            return None;
        }
        return Some(self.content.read().unwrap().clone());
    }

    fn click(&self, event: &I3Event) {
        if !self.connected.load(Ordering::Relaxed) {
            return;
        }
        let mut stream = self.stream.write().unwrap();
        #[allow(clippy::option_map_unit_fn)]
        stream.as_mut().map(|mut stream| {
            if serde_json::to_writer(&mut stream, &event).is_err() {
                eprintln!("Failed to write event to socket");
            }
            if stream.write_all("\n".as_bytes()).is_err() {
                eprintln!("Failed to write newline to socket");
            }
        });
    }
}
