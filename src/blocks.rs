pub mod battery_block;
mod bluetooth_battery;
pub mod date_block;
#[cfg(feature = "janne")]
pub mod default_route_block;
#[cfg(feature = "chris")]
pub mod disk_block;
pub mod dunst_block;
mod hidpp;
#[cfg(feature = "chris")]
pub mod ip_block;
#[cfg(feature = "janne")]
pub mod kimai_block;
pub mod load_block;
#[cfg(feature = "chris")]
pub mod memory_block;
pub mod socket_block;
pub mod temperature_block;
pub mod volume_block;

use std::fmt::{Display, Formatter};

use super::I3Event;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Markup {
    Pango,
}

impl Display for Markup {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pango => write!(f, "pango"),
        }
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct I3Block {
    pub full_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markup: Option<Markup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
}

pub trait Block {
    fn render(&self) -> Option<I3Block>;
    fn click(&self, event: &I3Event);
}
