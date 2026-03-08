//! SQLite-backed Cron Scheduler — persistent scheduled tasks.
//! Stores jobs in SQLite, ticks every 60s, spawns agent sessions for due jobs.

use std::str::FromStr;

use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// A cron job stored in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub task: String,
    pub channel: String,
    pub model: String,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
}

/// SQLite-backed cron scheduler.
pub struct CronScheduler {
    conn: Mutex<Connection>,
}

impl CronScheduler {
    /// Open or create the cron database at the given path.
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cron_jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                schedule TEXT NOT NULL,
                task TEXT NOT NULL,
                channel TEXT NOT NULL DEFAULT 'cli',
                model TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run TEXT,
                next_run TEXT
            );"
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Add a new cron job. Returns the job ID.
    pub fn add(&self, name: &str, schedule: &str, task: &str, channel: &str, model: &str) -> anyhow::Result<String> {
        // Validate cron expression
        let parsed = cron::Schedule::from_str(schedule)
            .map_err(|e| anyhow::anyhow!("Invalid cron expression: {}", e))?;

        let id = uuid::Uuid::new_v4().to_string();

        // Compute next run
        let next_run = parsed
            .upcoming(chrono::Utc)
            .next()
            .map(|t| t.to_rfc3339());

        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO cron_jobs (id, name, schedule, task, channel, model, enabled, next_run)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
            rusqlite::params![id, name, schedule, task, channel, model, next_run],
        )?;

        Ok(id)
    }

    /// List all cron jobs.
    pub fn list(&self) -> anyhow::Result<Vec<CronJob>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, task, channel, model, enabled, last_run, next_run FROM cron_jobs ORDER BY name"
        )?;
        let jobs = stmt.query_map([], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                task: row.get(3)?,
                channel: row.get(4)?,
                model: row.get(5)?,
                enabled: row.get::<_, i32>(6)? != 0,
                last_run: row.get(7)?,
                next_run: row.get(8)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(jobs)
    }

    /// Remove a cron job by ID or name.
    pub fn remove(&self, id_or_name: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock();
        let rows = conn.execute(
            "DELETE FROM cron_jobs WHERE id = ?1 OR name = ?1",
            rusqlite::params![id_or_name],
        )?;
        Ok(rows > 0)
    }

    /// Enable a cron job.
    pub fn enable(&self, id_or_name: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock();
        let rows = conn.execute(
            "UPDATE cron_jobs SET enabled = 1 WHERE id = ?1 OR name = ?1",
            rusqlite::params![id_or_name],
        )?;
        Ok(rows > 0)
    }

    /// Disable a cron job.
    pub fn disable(&self, id_or_name: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock();
        let rows = conn.execute(
            "UPDATE cron_jobs SET enabled = 0 WHERE id = ?1 OR name = ?1",
            rusqlite::params![id_or_name],
        )?;
        Ok(rows > 0)
    }

    /// Get due jobs (next_run <= now, enabled).
    pub fn due_jobs(&self) -> anyhow::Result<Vec<CronJob>> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, schedule, task, channel, model, enabled, last_run, next_run
             FROM cron_jobs WHERE enabled = 1 AND next_run IS NOT NULL AND next_run <= ?1"
        )?;
        let jobs = stmt.query_map(rusqlite::params![now], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                name: row.get(1)?,
                schedule: row.get(2)?,
                task: row.get(3)?,
                channel: row.get(4)?,
                model: row.get(5)?,
                enabled: row.get::<_, i32>(6)? != 0,
                last_run: row.get(7)?,
                next_run: row.get(8)?,
            })
        })?.filter_map(|r| r.ok()).collect();
        Ok(jobs)
    }

    /// Mark a job as just run and compute next_run.
    pub fn mark_run(&self, job_id: &str, schedule: &str) -> anyhow::Result<()> {
        let now = chrono::Utc::now();
        let next_run = cron::Schedule::from_str(schedule)
            .ok()
            .and_then(|s| s.upcoming(chrono::Utc).next())
            .map(|t| t.to_rfc3339());

        let conn = self.conn.lock();
        conn.execute(
            "UPDATE cron_jobs SET last_run = ?1, next_run = ?2 WHERE id = ?3",
            rusqlite::params![now.to_rfc3339(), next_run, job_id],
        )?;
        Ok(())
    }
}

/// A due job ready to execute (returned by the ticker).
#[derive(Debug, Clone)]
pub struct DueJob {
    pub job: CronJob,
}

/// Start the cron ticker as a background task. Returns a receiver for due jobs.
pub fn start_cron_ticker(
    scheduler: std::sync::Arc<CronScheduler>,
) -> (
    tokio::sync::mpsc::Receiver<DueJob>,
    std::sync::Arc<tokio::sync::Notify>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        // Skip immediate first tick
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match scheduler.due_jobs() {
                        Ok(jobs) => {
                            for job in jobs {
                                let schedule = job.schedule.clone();
                                let job_id = job.id.clone();

                                tracing::info!("Cron: job '{}' is due", job.name);

                                if tx.send(DueJob { job }).await.is_err() {
                                    return; // Receiver dropped
                                }

                                // Mark as run and compute next
                                let _ = scheduler.mark_run(&job_id, &schedule);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Cron ticker error: {}", e);
                        }
                    }
                }
                _ = shutdown_clone.notified() => {
                    tracing::info!("Cron ticker: shutting down");
                    break;
                }
            }
        }
    });

    (rx, shutdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_scheduler() -> CronScheduler {
        CronScheduler::new(":memory:").unwrap()
    }

    #[test]
    fn test_add_and_list() {
        let sched = test_scheduler();
        let id = sched.add("daily", "0 0 9 * * * *", "run daily report", "cli", "").unwrap();
        assert!(!id.is_empty());

        let jobs = sched.list().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "daily");
        assert_eq!(jobs[0].task, "run daily report");
    }

    #[test]
    fn test_invalid_cron() {
        let sched = test_scheduler();
        let result = sched.add("bad", "not a cron", "task", "cli", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove() {
        let sched = test_scheduler();
        sched.add("test", "0 0 9 * * * *", "task", "cli", "").unwrap();
        assert!(sched.remove("test").unwrap());
        assert_eq!(sched.list().unwrap().len(), 0);
    }

    #[test]
    fn test_enable_disable() {
        let sched = test_scheduler();
        let id = sched.add("test", "0 0 9 * * * *", "task", "cli", "").unwrap();

        sched.disable(&id).unwrap();
        let jobs = sched.list().unwrap();
        assert!(!jobs[0].enabled);

        sched.enable(&id).unwrap();
        let jobs = sched.list().unwrap();
        assert!(jobs[0].enabled);
    }
}
