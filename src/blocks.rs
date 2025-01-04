pub mod battery_block;
mod bluetooth_battery;
pub mod date_block;
pub mod default_route_block;
pub mod dunst_block;
mod hidpp;
pub mod load_block;
pub mod temperature_block;
pub mod volume_block;

use std::fmt::{Display, Formatter};

use super::I3Event;

#[derive(Debug, serde::Serialize)]
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

#[derive(Debug, Default, serde::Serialize)]
pub struct I3Block {
    pub full_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markup: Option<Markup>,
}

pub trait Block {
    fn render(&self) -> Option<I3Block>;
    fn click(&self, event: &I3Event);
}
