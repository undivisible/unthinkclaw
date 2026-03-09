//! unthinkclaw — Lightweight agent runtime
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
//! Swarms — Manager/Worker pattern for parallel execution
//! Plugins — JSON-RPC 2.0 extensibility
//! Cost — Token counting and billing (Phase 4)
//! Scheduler — Cron-based task automation (Phase 4)

pub mod agent;
pub mod channels;
pub mod claw_adapter;
pub mod config;
pub mod cost;
pub mod cron_scheduler;
pub mod embeddings;
pub mod gateway;
pub mod heartbeat;
pub mod mcp;
pub mod memory;
pub mod plugin;
pub mod prompt;
pub mod providers;
pub mod runtime;
pub mod scheduler;
pub mod skills;
pub mod swarm;
pub mod tools;

pub use agent::AgentRunner;
pub use channels::Channel;
pub use cost::CostTracker;
pub use scheduler::Scheduler;
pub use swarm::SwarmManager;

