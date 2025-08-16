#![deny(clippy::pedantic)]

use statusbar::{blocks::Block, I3Event};
use std::io::{BufRead as _, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::{mpsc, Arc, RwLock};
use std::time::Duration;

fn main() {
    // For cancellable sleep
    let (send, recv) = mpsc::channel::<()>();
    let sleep = Duration::from_secs(2);

    let block_name = std::env::args().nth(1).unwrap();
    let block: Box<dyn Block + Sync + Send> = match block_name.as_str() {
        "battery" => Box::new(statusbar::blocks::battery_block::BatteryBlock::new(
            &send.clone(),
        )),
        "kimai" => Box::new(statusbar::blocks::kimai_block::KimaiBlock::default()),
        _ => panic!("Unknown block"),
    };
    let block = Arc::new(block);

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR not set");
    let mut runtime_dir = Path::new(&runtime_dir).to_path_buf();
    runtime_dir.push("statusbar");
    if !runtime_dir.exists() {
        let _ = std::fs::create_dir_all(&runtime_dir);
    }
    runtime_dir.push(&block_name);
    if runtime_dir.exists() {
        let _ = std::fs::remove_file(&runtime_dir);
    }
    let socket = runtime_dir.to_str().unwrap().to_owned();
    let listener = UnixListener::bind(socket).expect("Unable to bind to unix socket");

    let consumers = Arc::new(RwLock::new(vec!()));
    let content = Arc::new(RwLock::new(String::new()));

    let consumers2 = consumers.clone();
    let content2 = content.clone();
    let block2 = block.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let consumers = consumers2.clone();
            let content = content2.clone();
            let block = block2.clone();
            if let Ok(mut stream) = stream {
                let stream2 = stream.try_clone().unwrap();
                std::thread::spawn(move || {
                    let (send, recv) = mpsc::channel::<()>();
                    consumers.write().unwrap().push(send.clone());
                    if stream.write_all(content.read().unwrap().as_bytes()).is_err() {
                        eprintln!("Lost client connection while performing initial write");
                        return;
                    }
                    while recv.recv().is_ok() {
                        if stream.write_all(content.read().unwrap().as_bytes()).is_err() {
                            eprintln!("Lost client connection while writing message");
                            return;
                        }
                    }
                });
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream2);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_ok() {
                            if line.is_empty() {
                                break;
                            }
                            if let Ok(content) = serde_json::from_str::<I3Event>(&line) {
                                block.click(&content);
                            } else {
                                eprintln!("Invalid event received from socket");
                            }
                        } else {
                            eprintln!("Failed to read event from socket");
                            break;
                        }
                    }
                });
            } else {
                eprintln!("Failed to accept stream");
            }
        }
    });

    let mut last = String::new();
    loop {
        let mut out = if let Some(output) = block.render() {
            serde_json::to_string(&output).unwrap()
        } else {
            String::from("{\"full_text\":\"\", \"name\":\"\"}") // Nothing to print for now
        };
        out.push('\n');
        if out != last {
            (*content.write().unwrap()).clone_from(&out);
            last = out;
            for send in consumers.read().unwrap().iter() {
                let _idc = send.send(());
            }
        }
        let _ = recv.recv_timeout(sleep);
    }
}
