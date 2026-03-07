//! LLM Provider abstraction — swap backends without changing agent logic.

pub mod traits;
pub mod anthropic;
pub mod openai_compat;
pub mod ollama;
pub mod oauth;

pub use traits::{ChatMessage, ChatRequest, ChatResponse, Provider, ToolCall};
