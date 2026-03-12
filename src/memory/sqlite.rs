//! SQLite-backed memory storage — all DB ops offloaded via spawn_blocking.

use async_trait::async_trait;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::traits::*;

#[derive(Clone)]
pub struct SqliteMemory {
    pool: Arc<ConnectionPool>,
}

struct ConnectionPool {
    connections: Vec<Mutex<Connection>>,
    next: AtomicUsize,
}

impl SqliteMemory {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let pool_size = if path == ":memory:" { 1 } else { 4 };
        let mut connections = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            let conn = Connection::open(path)?;
            initialize_connection(&conn)?;
            connections.push(Mutex::new(conn));
        }
        Ok(Self {
            pool: Arc::new(ConnectionPool {
                connections,
                next: AtomicUsize::new(0),
            }),
        })
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        Self::new(":memory:")
    }

    fn connection_index(&self) -> usize {
        let len = self.pool.connections.len();
        self.pool.next.fetch_add(1, Ordering::Relaxed) % len
    }

    fn build_fts_query(query: &str) -> Option<String> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
            .collect();
        if terms.is_empty() {
            None
        } else {
            Some(terms.join(" AND "))
        }
    }
}

fn initialize_connection(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        PRAGMA foreign_keys=ON;
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
        CREATE INDEX IF NOT EXISTS idx_conv_chat ON conversations(chat_id, timestamp, id);
        CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);",
    )?;
    Ok(())
}

fn parse_entry(row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
    let created_str: String = row.get(3)?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    Ok(MemoryEntry {
        key: row.get(0)?,
        value: row.get(1)?,
        metadata: row
            .get::<_, Option<String>>(2)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        created_at,
    })
}

#[async_trait]
impl MemoryBackend for SqliteMemory {
    async fn store(
        &self,
        namespace: &str,
        key: &str,
        value: &str,
        metadata: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let ns = namespace.to_string();
        let k = key.to_string();
        let v = value.to_string();
        let meta_str = metadata.map(|m| m.to_string());
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
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
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<MemoryEntry>> {
            let guard = pool.connections[index].lock();
            let mut stmt = guard.prepare(
                "SELECT key, value, metadata, created_at FROM memories WHERE namespace = ?1 AND key = ?2"
            )?;
            Ok(stmt.query_row(rusqlite::params![ns, k], parse_entry).ok())
        }).await?
    }

    async fn search(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let ns = namespace.to_string();
        let query = query.to_string();
        let fallback = format!("%{}%", query);
        let fts_query = Self::build_fts_query(&query);
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let guard = pool.connections[index].lock();
            if let Some(fts) = &fts_query {
                let mut stmt = guard.prepare(
                    "SELECT m.key, m.value, m.metadata, m.created_at
                     FROM memory_fts f
                     JOIN memories m USING(namespace, key)
                     WHERE f.namespace = ?1 AND memory_fts MATCH ?2
                     ORDER BY bm25(memory_fts), m.created_at DESC
                     LIMIT ?3",
                )?;
                let entries: Vec<MemoryEntry> = stmt
                    .query_map(rusqlite::params![ns, fts, limit], parse_entry)?
                    .filter_map(|r| r.ok())
                    .collect();
                if !entries.is_empty() {
                    return Ok(entries);
                }
            }

            let mut stmt = guard.prepare(
                "SELECT key, value, metadata, created_at FROM memories
                 WHERE namespace = ?1 AND (key LIKE ?2 OR value LIKE ?2)
                 ORDER BY created_at DESC LIMIT ?3",
            )?;
            let entries: Vec<MemoryEntry> = stmt
                .query_map(rusqlite::params![ns, fallback, limit], parse_entry)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(entries)
        })
        .await?
    }

    async fn forget(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
        let ns = namespace.to_string();
        let k = key.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
            guard.execute(
                "DELETE FROM memories WHERE namespace = ?1 AND key = ?2",
                rusqlite::params![ns, k],
            )?;
            guard.execute(
                "DELETE FROM memory_fts WHERE namespace = ?1 AND key = ?2",
                rusqlite::params![ns, k],
            )?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    async fn list(&self, namespace: &str) -> anyhow::Result<Vec<MemoryEntry>> {
        let ns = namespace.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<MemoryEntry>> {
            let guard = pool.connections[index].lock();
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

    async fn store_conversation(
        &self,
        chat_id: &str,
        sender_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let cid = chat_id.to_string();
        let sid = sender_id.to_string();
        let r = role.to_string();
        let c = content.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
            guard.execute(
                "INSERT INTO conversations (chat_id, sender_id, role, content) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![cid, sid, r, c],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn store_conversation_batch(
        &self,
        entries: &[(&str, &str, &str, &str)],
    ) -> anyhow::Result<()> {
        let entries: Vec<(String, String, String, String)> = entries
            .iter()
            .map(|(chat_id, sender_id, role, content)| {
                (
                    (*chat_id).to_string(),
                    (*sender_id).to_string(),
                    (*role).to_string(),
                    (*content).to_string(),
                )
            })
            .collect();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut guard = pool.connections[index].lock();
            let tx = guard.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO conversations (chat_id, sender_id, role, content) VALUES (?1, ?2, ?3, ?4)"
                )?;
                for (chat_id, sender_id, role, content) in entries {
                    stmt.execute(rusqlite::params![chat_id, sender_id, role, content])?;
                }
            }
            tx.commit()?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn get_conversation_history(
        &self,
        chat_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let cid = chat_id.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        let mut history = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<(String, String)>> {
            let guard = pool.connections[index].lock();
            let mut stmt = guard.prepare(
                "SELECT role, content FROM conversations WHERE chat_id = ?1 ORDER BY id DESC LIMIT ?2"
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
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
            let guard = pool.connections[index].lock();
            let mut stmt =
                guard.prepare("SELECT description FROM sticker_cache WHERE sticker_id = ?1")?;
            Ok(stmt
                .query_row(rusqlite::params![sid], |row| {
                    row.get::<_, Option<String>>(0)
                })
                .ok()
                .flatten())
        })
        .await?
    }

    async fn store_sticker_cache(
        &self,
        sticker_id: &str,
        file_id: &str,
        description: &str,
    ) -> anyhow::Result<()> {
        let sid = sticker_id.to_string();
        let fid = file_id.to_string();
        let desc = description.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
            guard.execute(
                "INSERT OR REPLACE INTO sticker_cache (sticker_id, file_id, description) VALUES (?1, ?2, ?3)",
                rusqlite::params![sid, fid, desc],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    // ── Embeddings ──

    async fn store_embedding(
        &self,
        namespace: &str,
        key: &str,
        vector: &[f32],
        text: &str,
    ) -> anyhow::Result<()> {
        let ns = namespace.to_string();
        let k = key.to_string();
        let blob: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
        let t = text.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
            guard.execute(
                "INSERT OR REPLACE INTO embeddings (namespace, key, vector, text) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![ns, k, blob, t],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn search_embeddings(
        &self,
        namespace: &str,
        query_vector: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<EmbeddingEntry>> {
        let ns = namespace.to_string();
        let qv = query_vector.to_vec();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<EmbeddingEntry>> {
            let guard = pool.connections[index].lock();
            let mut stmt = guard.prepare(
                "SELECT namespace, key, vector, text, created_at FROM embeddings WHERE namespace = ?1"
            )?;
            let rows: Vec<EmbeddingEntry> = stmt
                .query_map(rusqlite::params![ns], |row| {
                    let blob: Vec<u8> = row.get(2)?;
                    let vector: Vec<f32> = blob
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    let created_str: String = row.get(4)?;
                    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now());
                    Ok(EmbeddingEntry {
                        namespace: row.get(0)?,
                        key: row.get(1)?,
                        vector,
                        text: row.get(3)?,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            // Cosine similarity ranking
            let mut scored: Vec<(f32, EmbeddingEntry)> = rows
                .into_iter()
                .map(|entry| {
                    let sim = cosine_similarity_sqlite(&qv, &entry.vector);
                    (sim, entry)
                })
                .collect();
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(limit);
            Ok(scored.into_iter().map(|(_, e)| e).collect())
        }).await?
    }

    // ── File indexing ──

    async fn store_file_index(&self, path: &str, hash: &str) -> anyhow::Result<()> {
        let p = path.to_string();
        let h = hash.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
            guard.execute(
                "INSERT OR REPLACE INTO files (path, hash) VALUES (?1, ?2)",
                rusqlite::params![p, h],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn get_file_index(&self, path: &str) -> anyhow::Result<Option<FileIndex>> {
        let p = path.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<FileIndex>> {
            let guard = pool.connections[index].lock();
            let mut stmt = guard.prepare(
                "SELECT path, hash, last_indexed FROM files WHERE path = ?1"
            )?;
            Ok(stmt
                .query_row(rusqlite::params![p], |row| {
                    let created_str: String = row.get(2)?;
                    let last_indexed = chrono::DateTime::parse_from_rfc3339(&created_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now());
                    Ok(FileIndex {
                        path: row.get(0)?,
                        hash: row.get(1)?,
                        last_indexed,
                    })
                })
                .ok())
        }).await?
    }

    // ── Code chunks ──

    async fn store_chunk(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        content: &str,
        embedding: Option<&[f32]>,
    ) -> anyhow::Result<()> {
        let fp = file_path.to_string();
        let c = content.to_string();
        let blob: Option<Vec<u8>> = embedding.map(|e| e.iter().flat_map(|f| f.to_le_bytes()).collect());
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
            guard.execute(
                "INSERT INTO chunks (file_path, start_line, end_line, content, embedding) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![fp, start_line, end_line, c, blob],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }

    async fn get_chunks_for_file(&self, file_path: &str) -> anyhow::Result<Vec<Chunk>> {
        let fp = file_path.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Chunk>> {
            let guard = pool.connections[index].lock();
            let mut stmt = guard.prepare(
                "SELECT file_path, start_line, end_line, content, embedding, created_at
                 FROM chunks WHERE file_path = ?1 ORDER BY start_line ASC"
            )?;
            let rows: Vec<Chunk> = stmt
                .query_map(rusqlite::params![fp], |row| {
                    let blob: Option<Vec<u8>> = row.get(4)?;
                    let embedding = blob.map(|b| {
                        b.chunks_exact(4)
                            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                            .collect()
                    });
                    let created_str: String = row.get(5)?;
                    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now());
                    Ok(Chunk {
                        file_path: row.get(0)?,
                        start_line: row.get(1)?,
                        end_line: row.get(2)?,
                        content: row.get(3)?,
                        embedding,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        }).await?
    }

    async fn delete_chunks_for_file(&self, file_path: &str) -> anyhow::Result<()> {
        let fp = file_path.to_string();
        let pool = Arc::clone(&self.pool);
        let index = self.connection_index();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let guard = pool.connections[index].lock();
            guard.execute(
                "DELETE FROM chunks WHERE file_path = ?1",
                rusqlite::params![fp],
            )?;
            Ok(())
        }).await??;
        Ok(())
    }
}

fn cosine_similarity_sqlite(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let ma: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if ma == 0.0 || mb == 0.0 {
        return 0.0;
    }
    dot / (ma * mb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_and_recall() {
        let mem = SqliteMemory::in_memory().unwrap();
        mem.store("test", "greeting", "hello world", None)
            .await
            .unwrap();
        let entry = mem.recall("test", "greeting").await.unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().value, "hello world");
    }

    #[tokio::test]
    async fn test_search() {
        let mem = SqliteMemory::in_memory().unwrap();
        mem.store("test", "color", "blue is my favorite", None)
            .await
            .unwrap();
        mem.store("test", "food", "pizza is great", None)
            .await
            .unwrap();
        let results = mem.search("test", "favorite", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "color");
    }

    #[tokio::test]
    async fn test_conversation_history() {
        let mem = SqliteMemory::in_memory().unwrap();
        mem.store_conversation("chat1", "user1", "user", "hello")
            .await
            .unwrap();
        mem.store_conversation("chat1", "bot", "assistant", "hi there")
            .await
            .unwrap();
        let history = mem.get_conversation_history("chat1", 10).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].0, "user");
        assert_eq!(history[1].0, "assistant");
    }
}
