#![deny(clippy::pedantic)]

pub mod blocks;

/// An event received from I3
#[derive(Debug, Default, serde::Deserialize)]
pub struct I3Event {
    pub name: Option<String>,
    pub button: u8,
}
