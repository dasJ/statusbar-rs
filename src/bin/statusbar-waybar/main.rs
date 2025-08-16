#![deny(clippy::pedantic)]

use signal_hook::iterator::Signals;
use statusbar::{blocks::Block, I3Event};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn main() {
    // For cancellable sleep
    let (send, recv) = mpsc::channel::<()>();
    let sleep = Duration::from_secs(2);

    let block = std::env::args().nth(1).unwrap();
    let block: Box<dyn Block + Sync + Send> = match block.as_str() {
        "default-route" => {
            Box::new(statusbar::blocks::default_route_block::DefaultRouteBlock::default())
        }
        "dunst" => Box::new(statusbar::blocks::dunst_block::DunstBlock::new(
            send.clone(),
        )),
        "socket" => Box::new(statusbar::blocks::socket_block::SocketBlock::new(
            std::env::args().nth(2).unwrap(),
            send,
        )),
        "temperature" => {
            Box::new(statusbar::blocks::temperature_block::TemperatureBlock::default())
        }
        _ => panic!("Unknown blocK"),
    };

    // Set up mouse event handler
    let block = Arc::new(block);
    let block2 = Arc::clone(&block);
    if let Ok(mut signals) = Signals::new([35, 36, 37]) {
        std::thread::spawn(move || {
            for signal in signals.forever() {
                match signal {
                    // Left
                    35 => {
                        block2.click(&I3Event {
                            name: None,
                            button: 1,
                        });
                    }
                    // Middle
                    36 => {
                        block2.click(&I3Event {
                            name: None,
                            button: 2,
                        });
                    }
                    // Right
                    37 => {
                        block2.click(&I3Event {
                            name: None,
                            button: 3,
                        });
                    }
                    _ => {}
                }
            }
        });
    }

    let mut last = String::new();
    loop {
        let out = if let Some(output) = block.render() {
            serde_json::to_string(&serde_json::json!({
                "text": if let Some(color) = output.color {
                    format!("<span color='{color}'>{}</span>", output.full_text)
                } else { output.full_text },
                "tooltip": output.tooltip,
            }))
            .unwrap()
        } else {
            String::from("{\"text\":\"\"}") // Nothing to print for now
        };
        if last != out {
            println!("{out}");
            last = out;
        }
        let _ = recv.recv_timeout(sleep);
    }
}
