//! Tool abstraction — agent capabilities matching OpenClaw's tool set.
//!
//! Core tools: Read, Write, Edit, exec, web_search, web_fetch, memory_search, memory_get

pub mod traits;
pub mod shell;
pub mod file_ops;
pub mod edit;
pub mod web_search;
pub mod web_fetch;
pub mod vibemania;

pub use traits::{Tool, ToolSpec, ToolResult};
