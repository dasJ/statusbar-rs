use super::{Block, I3Block, I3Event};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    mpsc::{self, Sender},
    Arc, Mutex,
};
use zbus::blocking::{Connection, Proxy};

pub struct DunstBlock {
    paused_state: Option<Arc<AtomicBool>>,
    toggle_channel: Option<Mutex<Sender<()>>>,
}

impl Block for DunstBlock {
    fn render(&self) -> Option<I3Block> {
        let paused_state = self.paused_state.as_ref()?;

        if paused_state.load(Ordering::Relaxed) {
            Some(I3Block {
                full_text: "paused".to_owned(),
                color: Some("#ff0202".to_owned()),
                ..Default::default()
            })
        } else {
            Some(I3Block {
                full_text: "📢".to_owned(),
                ..Default::default()
            })
        }
    }

    fn click(&self, evt: &I3Event) {
        if evt.button == 3 {
            if let Some(channel) = &self.toggle_channel {
                let _idc = channel.lock().unwrap().send(());
            }
        }
    }
}

impl DunstBlock {
    #[must_use]
    pub fn new(timer_cancel: Sender<()>) -> Self {
        // Connect
        let Ok(dbus_conn) = Connection::session() else {
            return Self {
                paused_state: None,
                toggle_channel: None,
            };
        };

        // Build proxy
        let Ok(proxy) = Proxy::new(
            &dbus_conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.dunstproject.cmd0",
        ) else {
            return Self {
                paused_state: None,
                toggle_channel: None,
            };
        };

        // Query initial state
        let Ok(initial_value) = proxy.get_property::<bool>("paused") else {
            return Self {
                paused_state: None,
                toggle_channel: None,
            };
        };
        let value = Arc::new(AtomicBool::new(initial_value));

        // Query future signals
        let stream = proxy.receive_property_changed::<bool>("paused");
        let value2 = Arc::clone(&value);
        std::thread::spawn(move || {
            for item in stream {
                if let Ok(value) = item.get() {
                    value2.store(value, Ordering::Relaxed);
                    let _idc = timer_cancel.send(());
                }
            }
        });

        // Listen for commands
        let (send, receive) = mpsc::channel::<()>();
        let value2 = Arc::clone(&value);
        std::thread::spawn(move || {
            while receive.recv().is_ok() {
                let _dc = proxy.set_property::<bool>("paused", !value2.load(Ordering::Relaxed));
            }
        });

        Self {
            paused_state: Some(value),
            toggle_channel: Some(Mutex::new(send)),
        }
    }
}
