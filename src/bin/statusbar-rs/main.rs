#![deny(clippy::pedantic)]

use statusbar::{blocks::Block, I3Event};
use std::io::BufRead as _;
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Entrypoint
fn main() {
    // For cancellable sleep
    let (send, recv) = mpsc::channel::<()>();
    let sleep = Duration::from_secs(2);

    // Build blocks
    let blocks: Vec<Arc<dyn Block + Sync + Send>> = vec![
        Arc::new(statusbar::blocks::volume_block::VolumeBlock::new(
            send.clone(),
        )),
        #[cfg(feature = "chris")]
        Arc::<statusbar::blocks::memory_block::MemoryBlock>::default(),
        #[cfg(feature = "chris")]
        Arc::<statusbar::blocks::disk_block::DiskBlock>::default(),
        Arc::new(statusbar::blocks::battery_block::BatteryBlock::new(&send)),
        #[cfg(feature = "chris")]
        Arc::<statusbar::blocks::ip_block::IPBlock>::default(),
        #[cfg(feature = "janne")]
        Arc::<statusbar::blocks::default_route_block::DefaultRouteBlock>::default(),
        Arc::new(statusbar::blocks::dunst_block::DunstBlock::new(send)),
        #[cfg(feature = "janne")]
        Arc::new(statusbar::blocks::kimai_block::KimaiBlock::default()),
        Arc::<statusbar::blocks::load_block::LoadBlock>::default(),
        Arc::<statusbar::blocks::temperature_block::TemperatureBlock>::default(),
        Arc::<statusbar::blocks::date_block::DateBlock>::default(),
    ];

    // Header block
    println!(
        "{}",
        serde_json::json!({
            "version": 1,
            "stop_signal": 19,
            "cont_signal": 18,
            "click_events": true,
        })
    );

    // Begin infinite JSON stream
    println!("[");
    let mut out = Vec::with_capacity(blocks.len());

    // Set up mouse event handler
    let blocks2 = blocks.iter().map(Arc::clone).collect();
    std::thread::spawn(move || {
        event_handler(blocks2);
    });

    // Loop forever over all blocks
    loop {
        for (index, block) in blocks.iter().enumerate() {
            // Allow skipping blocks
            if let Some(mut output) = block.render() {
                output.name = index.to_string();
                out.push(output);
            }
        }
        // Output all blocks
        println!("{},", serde_json::to_string(&out).unwrap());
        // Reset and wait before restarting loop
        out.clear();
        let _ = recv.recv_timeout(sleep);
    }
}

/// Handles I3 mouse events
#[allow(clippy::needless_pass_by_value)]
fn event_handler(blocks: Vec<Arc<dyn Block + Sync + Send>>) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        // Pretty much I3's "hello"
        if line == "[" || line.is_empty() {
            continue;
        }
        // Handle the event
        if let Ok(event) = serde_json::from_str::<I3Event>(line.strip_prefix(',').unwrap_or(&line))
        {
            if let Some(ref name) = event.name {
                if let Ok(name) = name.parse::<usize>() {
                    if let Some(block) = blocks.get(name) {
                        block.click(&event);
                    } else {
                        eprintln!("Got event for invalid block from i3: {name}");
                    }
                } else {
                    eprintln!("Received invalid block name from i3: {name}");
                }
            } else {
                eprintln!("Received event without name from i3");
            }
        } else {
            eprintln!("Received invalid JSON from i3: {line}");
        }
    }
}
