//! Core MemoryBackend trait.

use async_trait::async_trait;
use serde_json::Value;

/// Memory entry
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The core MemoryBackend trait.
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Store a key-value memory
    async fn store(&self, namespace: &str, key: &str, value: &str, metadata: Option<Value>) -> anyhow::Result<()>;

    /// Recall a specific memory by key
    async fn recall(&self, namespace: &str, key: &str) -> anyhow::Result<Option<MemoryEntry>>;

    /// Search memories by query (semantic or keyword)
    async fn search(&self, namespace: &str, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Delete a memory
    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<()>;

    /// List all memories in a namespace
    async fn list(&self, namespace: &str) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Store a conversation message
    async fn store_conversation(&self, chat_id: &str, sender_id: &str, role: &str, content: &str) -> anyhow::Result<()>;

    /// Get recent conversation history (returns role, content pairs)
    async fn get_conversation_history(&self, chat_id: &str, limit: usize) -> anyhow::Result<Vec<(String, String)>>;

    /// Get cached sticker description by ID
    async fn get_sticker_cache(&self, sticker_id: &str) -> anyhow::Result<Option<String>>;

    /// Store sticker cache (sticker_id → description)
    async fn store_sticker_cache(&self, sticker_id: &str, file_id: &str, description: &str) -> anyhow::Result<()>;
}
