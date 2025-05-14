use chrono::{DateTime, Days, Local, NaiveTime, TimeDelta};
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

/// The state we retrieve from Kimai
struct KimaiState {
    /// Timestamp when the data was sampled
    timestamp: DateTime<Local>,
    /// Number of seconds of timesheets today that are finished
    duration_today: i64,
    /// If a timesheet is active, the duration of said timesheet at the time of sampling,
    /// otherwise is 0
    active_timesheet: i64,
}

enum CurrentState {
    /// No data has been received yet
    NoData,
    /// An internal error has occured
    Error,
    /// Duration is available, sampled at the given time
    DurationAvailable(KimaiState),
}

pub struct KimaiBlock {
    current_state: Arc<RwLock<CurrentState>>,
}

impl Block for KimaiBlock {
    fn render(&self) -> Option<I3Block> {
        let state = self.current_state.read().unwrap();
        let state = match &*state {
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

        // Duration of the active timesheet
        let active_duration = if state.active_timesheet != 0 {
            Local::now()
                .signed_duration_since(state.timestamp)
                .checked_add(&TimeDelta::seconds(state.active_timesheet))
                .unwrap()
                .num_seconds()
        } else {
            0
        };

        let full_time_today = seconds_to_timestamp(state.duration_today + active_duration);

        Some(I3Block {
            full_text: if active_duration > 0 {
                format!(
                    "<span foreground='#02ff02'>{} ({})</span>",
                    seconds_to_timestamp(active_duration),
                    full_time_today
                )
            } else {
                full_time_today.clone()
            },
            short_text: Some(full_time_today),
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
        duration: i64,
        begin: String,
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
        let sample_time = Local::now();
        let from = sample_time
            .checked_sub_days(Days::new(1))
            .unwrap_or_else(Local::now)
            .with_time(NaiveTime::from_hms_opt(23, 0, 0).unwrap_or_default())
            .single()
            .unwrap_or_else(Local::now);
        let mut resp = match http_agent
            .get(format!("{}/api/timesheets", cfg.base_url))
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

        let mut active_timesheet_duration: i64 = 0;
        let duration: i64 = json
            .into_iter()
            .filter_map(|ts| {
                if ts.duration != 0 {
                    return Some(ts.duration);
                }
                active_timesheet_duration += Local::now()
                    .fixed_offset()
                    .signed_duration_since(
                        DateTime::parse_from_str(&ts.begin, "%Y-%m-%dT%H:%M:%S%z").ok()?,
                    )
                    .num_seconds()
                    .abs();
                None
            })
            .sum();
        let state = KimaiState {
            timestamp: sample_time,
            duration_today: duration,
            active_timesheet: active_timesheet_duration,
        };
        *(current_state.write().unwrap()) = CurrentState::DurationAvailable(state);

        let hours = ((duration + active_timesheet_duration) / 60) / 60; // implicit floor
        if let Some(notify) = cfg.notify_daily_hours {
            if hours < notify.into() {
                notified = false;
            } else if hours >= notify.into() && !notified && active_timesheet_duration > 0 {
                if let Ok(Ok(ref dbus_notifier)) = dbus_notifier {
                    send_notification(dbus_notifier, notify);
                }
                notified = true;
            }
        }

        std::thread::sleep(Duration::from_secs(300));
    }
}

fn seconds_to_timestamp(seconds: i64) -> String {
    let hours = (seconds / 60) / 60; // implicit floor
    let mut minutes = seconds / 60 % 60;
    if seconds % 60 > 0 {
        minutes += 1;
    }
    format!("{hours}:{minutes:0>2}")
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
