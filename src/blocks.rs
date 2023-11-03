pub mod battery_block;
mod bluetooth_battery;
pub mod date_block;
pub mod default_route_block;
pub mod dunst_block;
mod hidpp;
pub mod load_block;
pub mod temperature_block;
pub mod volume_block;

use super::I3Event;

#[derive(Debug, serde::Serialize)]
pub enum Markup {
    Pango,
}

impl ToString for Markup {
    fn to_string(&self) -> String {
        match self {
            Self::Pango => "pango".to_owned(),
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
