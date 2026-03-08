//! SQLite-backed memory storage — all DB ops offloaded via spawn_blocking.

use async_trait::async_trait;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::sync::Arc;

use super::traits::*;

#[derive(Clone)]
pub struct SqliteMemory {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteMemory {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS memories (
                namespace TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                metadata TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (namespace, key)
            );
            CREATE TABLE IF NOT EXISTS embeddings (
                namespace TEXT NOT NULL,
                key TEXT NOT NULL,
                vector BLOB NOT NULL,
                text TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (namespace, key),
                FOREIGN KEY (namespace, key) REFERENCES memories(namespace, key) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS conversations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_id TEXT NOT NULL,
                sender_id TEXT,
                sender_name TEXT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                metadata TEXT
            );
            CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                hash TEXT NOT NULL,
                last_indexed TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(namespace, key, value);
            CREATE TABLE IF NOT EXISTS sticker_cache (
                sticker_id TEXT PRIMARY KEY,
                file_id TEXT NOT NULL,
                description TEXT,
                analyzed_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_memories_ns ON memories(namespace);
            CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_embeddings_ns ON embeddings(namespace);
            CREATE INDEX IF NOT EXISTS idx_conv_chat ON conversations(chat_id, timestamp);
            CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);"
        )?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        Self::new(":memory:")
    }
}

fn parse_entry(row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
    let created_str: String = row.get(3)?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    Ok(MemoryEntry {
        key: row.get(0)?,
        value: row.get(1)?,
        metadata: row.get::<_, Option<String>>(2)?.and_then(|s| serde_json::from_str(&s).ok()),
        created_at,
    })
}

#[async_trait]
impl MemoryBackend for SqliteMemory {
    async fn store(&self, namespace: &str, key: &str, value: &str, metadata: Option<serde_json::Value>) -> anyhow::Result<()> {
        let ns = namespace.to_string();
        let k = key.to_string();
        let v = value.to_string();
        let meta_str = metadata.map(|m| m.to_string());
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = conn.lock();
            guard.execute(
                "INSERT OR REPLACE INTO memories (namespace, key, value, metadata) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![ns, k, v, meta_str],
            )?;
            guard.execute(
                "INSERT OR REPLACE INTO memory_fts (namespace, key, value) VALUES (?1, ?2, ?3)",
                rusqlite::params![ns, k, v],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn recall(&self, namespace: &str, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        let ns = namespace.to_string();
        let k = key.to_string();
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<MemoryEntry>> {
            let guard = conn.lock();
            let mut stmt = guard.prepare(
                "SELECT key, value, metadata, created_at FROM memories WHERE namespace = ?1 AND key = ?2"
            )?;
            Ok(stmt.query_row(rusqlite::params![ns, k], parse_entry).ok())
        }).await?
    }

    async fn search(&self, namespace: &str, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let ns = namespace.to_string();
        let pattern = format!("%{}%", query);
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let guard = conn.lock();
            let mut stmt = guard.prepare(
                "SELECT key, value, metadata, created_at FROM memories \
                 WHERE namespace = ?1 AND (key LIKE ?2 OR value LIKE ?2) \
                 ORDER BY created_at DESC LIMIT ?3"
            )?;
            let entries: Vec<MemoryEntry> = stmt
                .query_map(rusqlite::params![ns, pattern, limit], parse_entry)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(entries)
        }).await?
    }

    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
        let ns = namespace.to_string();
        let k = key.to_string();
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = conn.lock();
            guard.execute("DELETE FROM memories WHERE namespace = ?1 AND key = ?2", rusqlite::params![ns, k])?;
            guard.execute("DELETE FROM memory_fts WHERE namespace = ?1 AND key = ?2", rusqlite::params![ns, k])?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn list(&self, namespace: &str) -> anyhow::Result<Vec<MemoryEntry>> {
        let ns = namespace.to_string();
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let guard = conn.lock();
            let mut stmt = guard.prepare(
                "SELECT key, value, metadata, created_at FROM memories WHERE namespace = ?1 ORDER BY created_at DESC"
            )?;
            let entries: Vec<MemoryEntry> = stmt
                .query_map(rusqlite::params![ns], parse_entry)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(entries)
        }).await?
    }

    async fn store_conversation(&self, chat_id: &str, sender_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
        let cid = chat_id.to_string();
        let sid = sender_id.to_string();
        let r = role.to_string();
        let c = content.to_string();
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = conn.lock();
            guard.execute(
                "INSERT INTO conversations (chat_id, sender_id, role, content) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![cid, sid, r, c],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn get_conversation_history(&self, chat_id: &str, limit: usize) -> anyhow::Result<Vec<(String, String)>> {
        let cid = chat_id.to_string();
        let conn = Arc::clone(&self.conn);
        let mut history = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<(String, String)>> {
            let guard = conn.lock();
            let mut stmt = guard.prepare(
                "SELECT role, content FROM conversations WHERE chat_id = ?1 ORDER BY timestamp DESC LIMIT ?2"
            )?;
            let rows: Vec<(String, String)> = stmt
                .query_map(rusqlite::params![cid, limit], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        }).await??;
        history.reverse();
        Ok(history)
    }

    async fn get_sticker_cache(&self, sticker_id: &str) -> anyhow::Result<Option<String>> {
        let sid = sticker_id.to_string();
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
            let guard = conn.lock();
            let mut stmt = guard.prepare("SELECT description FROM sticker_cache WHERE sticker_id = ?1")?;
            Ok(stmt.query_row(rusqlite::params![sid], |row| row.get::<_, Option<String>>(0)).ok().flatten())
        }).await?
    }

    async fn store_sticker_cache(&self, sticker_id: &str, file_id: &str, description: &str) -> anyhow::Result<()> {
        let sid = sticker_id.to_string();
        let fid = file_id.to_string();
        let desc = description.to_string();
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = conn.lock();
            guard.execute(
                "INSERT OR REPLACE INTO sticker_cache (sticker_id, file_id, description) VALUES (?1, ?2, ?3)",
                rusqlite::params![sid, fid, desc],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_and_recall() {
        let mem = SqliteMemory::in_memory().unwrap();
        mem.store("test", "greeting", "hello world", None).await.unwrap();
        let entry = mem.recall("test", "greeting").await.unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().value, "hello world");
    }

    #[tokio::test]
    async fn test_search() {
        let mem = SqliteMemory::in_memory().unwrap();
        mem.store("test", "color", "blue is my favorite", None).await.unwrap();
        mem.store("test", "food", "pizza is great", None).await.unwrap();
        let results = mem.search("test", "favorite", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "color");
    }

    #[tokio::test]
    async fn test_conversation_history() {
        let mem = SqliteMemory::in_memory().unwrap();
        mem.store_conversation("chat1", "user1", "user", "hello").await.unwrap();
        mem.store_conversation("chat1", "bot", "assistant", "hi there").await.unwrap();
        let history = mem.get_conversation_history("chat1", 10).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].0, "user");
        assert_eq!(history[1].0, "assistant");
    }
}
