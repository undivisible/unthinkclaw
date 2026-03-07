//! aclaw — Lightweight agent runtime
//! Successor to OpenClaw. Best-of-breed from ZeroClaw, NanoClaw, HiClaw.
//!
//! Core traits (all swappable):
//! - `Provider` — LLM backend (Anthropic, OpenAI, Gemini, Ollama, OpenRouter, Groq)
//! - `Channel` — Communication (CLI, Telegram, Discord, Matrix, WebSocket)
//! - `Tool` — Agent capability (Shell, File I/O, Vibemania, custom)
//! - `MemoryBackend` — Persistent state (SQLite, vector embeddings, file-based)
//! - `RuntimeAdapter` — Execution (Native, Docker, WASM planned)
//!
//! Gateway — HTTP/WebSocket for remote management
//! Embeddings — Vector search for semantic memory

pub mod agent;
pub mod channels;
pub mod claw_adapter;
pub mod config;
pub mod embeddings;
pub mod gateway;
pub mod memory;
pub mod plugin;
pub mod providers;
pub mod runtime;
pub mod swarm;
pub mod tools;
