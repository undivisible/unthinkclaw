//! Channel abstraction — communication interfaces
//! Enable only what you need via Cargo features.

pub mod traits;

// Core channels (always available or feature-gated)
#[cfg(feature = "channel-cli")]
pub mod cli;
#[cfg(feature = "channel-telegram")]
pub mod telegram;
#[cfg(feature = "channel-discord")]
pub mod discord;
#[cfg(feature = "channel-slack")]
pub mod slack;
#[cfg(feature = "channel-whatsapp")]
pub mod whatsapp;
#[cfg(feature = "channel-signal")]
pub mod signal;
#[cfg(feature = "channel-matrix")]
pub mod matrix;
#[cfg(feature = "channel-irc")]
pub mod irc;
#[cfg(feature = "channel-googlechat")]
pub mod googlechat;
#[cfg(feature = "channel-msteams")]
pub mod msteams;

pub use traits::{Channel, IncomingMessage, OutgoingMessage};

