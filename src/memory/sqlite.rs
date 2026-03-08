//! SQLite-backed memory storage.

use async_trait::async_trait;
use parking_lot::Mutex;
use rusqlite::Connection;

use super::traits::*;

pub struct SqliteMemory {
    conn: Mutex<Connection>,
}

impl SqliteMemory {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        // Create parent directories if they don't exist
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
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
            CREATE INDEX IF NOT EXISTS idx_memories_ns ON memories(namespace);
            CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_embeddings_ns ON embeddings(namespace);"
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        Self::new(":memory:")
    }

    /// Store an embedding vector for semantic search
    pub fn store_embedding(&self, namespace: &str, key: &str, vector: &[f32], text: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        let vector_bytes: Vec<u8> = vector
            .iter()
            .flat_map(|f| f.to_le_bytes().to_vec())
            .collect();
        
        conn.execute(
            "INSERT OR REPLACE INTO embeddings (namespace, key, vector, text) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![namespace, key, vector_bytes, text],
        )?;
        Ok(())
    }

    /// Get embedding for a key
    pub fn recall_embedding(&self, namespace: &str, key: &str) -> anyhow::Result<Option<(Vec<f32>, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT vector, text FROM embeddings WHERE namespace = ?1 AND key = ?2"
        )?;
        
        let result = stmt.query_row(rusqlite::params![namespace, key], |row| {
            let vector_bytes: Vec<u8> = row.get(0)?;
            let text: String = row.get(1)?;
            
            // Convert bytes back to f32 vector
            let vector: Vec<f32> = vector_bytes
                .chunks_exact(4)
                .map(|chunk| {
                    let arr = [chunk[0], chunk[1], chunk[2], chunk[3]];
                    f32::from_le_bytes(arr)
                })
                .collect();
            
            Ok((vector, text))
        }).ok();
        
        Ok(result)
    }
}

#[async_trait]
impl MemoryBackend for SqliteMemory {
    async fn store(&self, namespace: &str, key: &str, value: &str, metadata: Option<serde_json::Value>) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        let meta_str = metadata.map(|m| m.to_string());
        conn.execute(
            "INSERT OR REPLACE INTO memories (namespace, key, value, metadata) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![namespace, key, value, meta_str],
        )?;
        Ok(())
    }

    async fn recall(&self, namespace: &str, key: &str) -> anyhow::Result<Option<MemoryEntry>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT key, value, metadata, created_at FROM memories WHERE namespace = ?1 AND key = ?2"
        )?;
        let result = stmt.query_row(rusqlite::params![namespace, key], |row| {
            Ok(MemoryEntry {
                key: row.get(0)?,
                value: row.get(1)?,
                metadata: row.get::<_, Option<String>>(2)?.and_then(|s| serde_json::from_str(&s).ok()),
                created_at: chrono::Utc::now(), // Simplified
            })
        }).ok();
        Ok(result)
    }

    async fn search(&self, namespace: &str, query: &str, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT key, value, metadata, created_at FROM memories WHERE namespace = ?1 AND (key LIKE ?2 OR value LIKE ?2) ORDER BY created_at DESC LIMIT ?3"
        )?;
        let entries = stmt.query_map(rusqlite::params![namespace, pattern, limit], |row| {
            Ok(MemoryEntry {
                key: row.get(0)?,
                value: row.get(1)?,
                metadata: row.get::<_, Option<String>>(2)?.and_then(|s| serde_json::from_str(&s).ok()),
                created_at: chrono::Utc::now(),
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(entries)
    }

    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM memories WHERE namespace = ?1 AND key = ?2", rusqlite::params![namespace, key])?;
        Ok(())
    }

    async fn list(&self, namespace: &str) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT key, value, metadata, created_at FROM memories WHERE namespace = ?1 ORDER BY created_at DESC"
        )?;
        let entries = stmt.query_map(rusqlite::params![namespace], |row| {
            Ok(MemoryEntry {
                key: row.get(0)?,
                value: row.get(1)?,
                metadata: row.get::<_, Option<String>>(2)?.and_then(|s| serde_json::from_str(&s).ok()),
                created_at: chrono::Utc::now(),
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(entries)
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
}
