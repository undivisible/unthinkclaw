//! Tool abstraction — agent capabilities matching OpenClaw's tool set.
//!
//! Core tools (OpenClaw parity):
//!   group:runtime  — exec (shell commands)
//!   group:fs       — Read, Write, Edit
//!   group:web      — web_search, web_fetch
//!   group:memory   — memory_search, memory_get
//!   group:sessions — session_status, list_models
//!   group:messaging — message (Telegram send/edit/delete)

pub mod traits;
pub mod shell;
pub mod file_ops;
pub mod edit;
pub mod web_search;
pub mod web_fetch;
pub mod session;
pub mod message;
pub mod dynamic;
pub mod vibemania;
pub mod browser;
pub mod mcp;

pub use traits::{Tool, ToolSpec, ToolResult};
