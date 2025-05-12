use chrono::{DateTime, Days, Local, NaiveTime};
use ureq::Agent;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

use super::{Block, I3Block, I3Event, Markup};
use std::collections::HashMap;
use std::str::FromStr as _;
use std::sync::{Arc, RwLock};
use std::time::Duration;

struct Config {
    base_url: String,
    token: String,
    notify_daily_hours: Option<u8>,
}

enum CurrentState {
    /// No data has been received yet
    NoData,
    /// An internal error has occured
    Error,
    /// Duration is available and whether a timesheet is currently active
    DurationAvailable((String, bool)),
}

pub struct KimaiBlock {
    current_state: Arc<RwLock<CurrentState>>,
}

impl Block for KimaiBlock {
    fn render(&self) -> Option<I3Block> {
        let state = self.current_state.read().unwrap();
        let (duration, timesheet_is_active) = match &*state {
            CurrentState::NoData => return None,
            CurrentState::Error => {
                return Some(I3Block {
                    full_text: "ERROR".to_owned(),
                    color: Some("#ff0202".to_owned()),
                    ..Default::default()
                });
            }
            CurrentState::DurationAvailable(data) => data,
        };

        Some(I3Block {
            full_text: if *timesheet_is_active {
                format!("<span foreground='#02ff02'>{duration}</span>")
            } else {
                duration.clone()
            },
            markup: Some(Markup::Pango),
            ..Default::default()
        })
    }

    fn click(&self, _: &I3Event) {}
}

impl Default for KimaiBlock {
    fn default() -> Self {
        let current_state = Arc::new(RwLock::new(CurrentState::NoData));
        // Try to parse config
        let Some(config_file) = xdg::BaseDirectories::default().get_config_file("kimai") else {
            return Self { current_state };
        };
        let Ok(cfg) = env_file_reader::read_file(config_file) else {
            eprintln!("Unable to parse Kimai config");
            return Self { current_state };
        };
        let Some(base_url) = cfg.get("kimaiURL").map(ToString::to_string) else {
            eprintln!("No Kimai base URL found");
            return Self { current_state };
        };
        let Some(token) = cfg.get("token").map(ToString::to_string) else {
            eprintln!("No Kimai token found");
            return Self { current_state };
        };
        let notify_daily_hours = cfg
            .get("notifyDailyHours")
            .and_then(|s| u8::from_str(s).ok());

        let cfg = Config {
            base_url,
            token,
            notify_daily_hours,
        };
        // Background thread
        let state2 = current_state.clone();
        std::thread::spawn(move || request_thread(&cfg, &state2));

        Self { current_state }
    }
}

fn request_thread(cfg: &Config, current_state: &Arc<RwLock<CurrentState>>) {
    #[derive(Debug, serde::Deserialize)]
    struct Timesheet {
        duration: u64,
        begin: String,
        end: Option<String>,
    }

    let http_agent: Agent = Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .build(),
        )
        .https_only(true)
        .user_agent("statusbar-rs")
        .accept("application/json")
        .build()
        .into();

    let mut notified = false;
    let dbus_notifier = Connection::session().map(|conn| {
        Proxy::new(
            &conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
    });

    loop {
        let from = Local::now()
            .checked_sub_days(Days::new(1))
            .unwrap_or_else(Local::now)
            .with_time(NaiveTime::from_hms_opt(23, 0, 0).unwrap_or_default())
            .single()
            .unwrap_or_else(Local::now);
        let mut resp = match http_agent
            .get(format!("{}/api/timesheets/recent", cfg.base_url))
            .header("Authorization", format!("Bearer {}", cfg.token))
            .query("begin", from.format("%Y-%m-%dT%H:%M:%S").to_string())
            .query("size", "20000")
            .call()
        {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("Error calling Kimai API: {e}");
                *(current_state.write().unwrap()) = CurrentState::Error;
                return;
            }
        };

        let json: Vec<_> = match resp.body_mut().read_json::<Vec<Timesheet>>() {
            Ok(json) => json,
            Err(e) => {
                eprintln!("Error deserializing Kimai API: {e}");
                *(current_state.write().unwrap()) = CurrentState::Error;
                return;
            }
        };

        let mut timesheet_is_active = false;
        let duration: u64 = json
            .into_iter()
            .filter_map(|ts| {
                if ts.duration != 0 {
                    return Some(ts.duration);
                }
                timesheet_is_active = true;
                Some(
                    (if let Some(end) = &ts.end {
                        DateTime::parse_from_str(end, "%Y-%m-%dT%H:%M:%S%z").ok()?
                    } else {
                        Local::now().fixed_offset()
                    })
                    .signed_duration_since(
                        DateTime::parse_from_str(&ts.begin, "%Y-%m-%dT%H:%M:%S%z").ok()?,
                    )
                    .num_seconds()
                    .abs()
                    .try_into()
                    .expect("i64 somehow didn't fit into u64 after abs()"),
                )
            })
            .sum();
        let hours = (duration / 60) / 60; // implicit floor
        let mut minutes = duration / 60 % 60;
        if duration % 60 > 0 {
            minutes += 1;
        }
        *(current_state.write().unwrap()) = CurrentState::DurationAvailable((
            format!("{hours}:{minutes:0>2}"),
            timesheet_is_active,
        ));

        if let Some(notify) = cfg.notify_daily_hours {
            if hours < notify.into() {
                notified = false;
            } else if hours >= notify.into() && !notified && timesheet_is_active {
                if let Ok(Ok(ref dbus_notifier)) = dbus_notifier {
                    send_notification(dbus_notifier, notify);
                }
                notified = true;
            }
        }

        std::thread::sleep(Duration::from_secs(120));
    }
}

fn send_notification(proxy: &Proxy, hours: u8) {
    let _ = proxy.call_noreply(
        "Notify",
        &(
            "kimai",
            0u32,
            "dialog-information",
            "Enough work",
            format!("You have reached your daily {hours}h"),
            vec![""; 0],
            HashMap::<&str, &Value>::new(),
            0,
        ),
    );
}
