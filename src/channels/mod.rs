//! Channel abstraction — communication interfaces
//! Support for: CLI, Telegram, Discord, WebSocket

pub mod traits;
pub mod cli;
pub mod telegram;
pub mod discord;

pub use traits::{Channel, IncomingMessage, OutgoingMessage};



