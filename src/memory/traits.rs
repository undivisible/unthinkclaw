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

/// Embedding entry (vector + source text)
#[derive(Debug, Clone)]
pub struct EmbeddingEntry {
    pub namespace: String,
    pub key: String,
    pub vector: Vec<f32>,
    pub text: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Indexed file record
#[derive(Debug, Clone)]
pub struct FileIndex {
    pub path: String,
    pub hash: String,
    pub last_indexed: chrono::DateTime<chrono::Utc>,
}

/// Code chunk with optional embedding
#[derive(Debug, Clone)]
pub struct Chunk {
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The core MemoryBackend trait.
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Store a key-value memory
    async fn store(
        &self,
        namespace: &str,
        key: &str,
        value: &str,
        metadata: Option<Value>,
    ) -> anyhow::Result<()>;

    /// Recall a specific memory by key
    async fn recall(&self, namespace: &str, key: &str) -> anyhow::Result<Option<MemoryEntry>>;

    /// Search memories by query (semantic or keyword)
    async fn search(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Delete a memory
    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<()>;

    /// List all memories in a namespace
    async fn list(&self, namespace: &str) -> anyhow::Result<Vec<MemoryEntry>>;

    /// Store a conversation message
    async fn store_conversation(
        &self,
        chat_id: &str,
        sender_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<()>;

    /// Store multiple conversation messages in one batch.
    async fn store_conversation_batch(
        &self,
        entries: &[(&str, &str, &str, &str)],
    ) -> anyhow::Result<()> {
        for (chat_id, sender_id, role, content) in entries {
            self.store_conversation(chat_id, sender_id, role, content)
                .await?;
        }
        Ok(())
    }

    /// Get recent conversation history (returns role, content pairs)
    async fn get_conversation_history(
        &self,
        chat_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, String)>>;

    /// Get cached sticker description by ID
    async fn get_sticker_cache(&self, sticker_id: &str) -> anyhow::Result<Option<String>>;

    /// Store sticker cache (sticker_id → description)
    async fn store_sticker_cache(
        &self,
        sticker_id: &str,
        file_id: &str,
        description: &str,
    ) -> anyhow::Result<()>;

    // ── Embeddings ──

    /// Store a vector embedding for a memory key
    async fn store_embedding(
        &self,
        namespace: &str,
        key: &str,
        vector: &[f32],
        text: &str,
    ) -> anyhow::Result<()> {
        let _ = (namespace, key, vector, text);
        Ok(())
    }

    /// Search embeddings by cosine similarity (returns nearest matches)
    async fn search_embeddings(
        &self,
        namespace: &str,
        query_vector: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<EmbeddingEntry>> {
        let _ = (namespace, query_vector, limit);
        Ok(Vec::new())
    }

    // ── File indexing ──

    /// Record that a file has been indexed
    async fn store_file_index(&self, path: &str, hash: &str) -> anyhow::Result<()> {
        let _ = (path, hash);
        Ok(())
    }

    /// Get file index entry (to check if re-indexing is needed)
    async fn get_file_index(&self, path: &str) -> anyhow::Result<Option<FileIndex>> {
        let _ = path;
        Ok(None)
    }

    // ── Code chunks ──

    /// Store a code chunk with optional embedding
    async fn store_chunk(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        content: &str,
        embedding: Option<&[f32]>,
    ) -> anyhow::Result<()> {
        let _ = (file_path, start_line, end_line, content, embedding);
        Ok(())
    }

    /// Search chunks by file path
    async fn get_chunks_for_file(&self, file_path: &str) -> anyhow::Result<Vec<Chunk>> {
        let _ = file_path;
        Ok(Vec::new())
    }

    /// Delete all chunks for a file (before re-indexing)
    async fn delete_chunks_for_file(&self, file_path: &str) -> anyhow::Result<()> {
        let _ = file_path;
        Ok(())
    }
}
