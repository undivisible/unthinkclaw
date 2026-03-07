//! Agent Swarms — Manager/Worker pattern for parallel autonomous execution
//! Core to Vibemania: orchestrate multiple agents, track progress, distribute work

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// Task to be executed by workers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub goal: String,
    pub priority: u8, // 0-10, higher = more urgent
    pub status: TaskStatus,
    pub assigned_to: Option<String>, // worker_id
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Assigned,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Worker agent state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worker {
    pub id: String,
    pub model: String,
    pub status: WorkerStatus,
    pub current_task: Option<String>,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkerStatus {
    Idle,
    Busy,
    Error,
    Offline,
}

/// Manager coordinates workers
pub struct SwarmManager {
    tasks: Arc<RwLock<Vec<Task>>>,
    workers: Arc<RwLock<Vec<Worker>>>,
    task_tx: mpsc::UnboundedSender<Task>,
    task_rx: Arc<RwLock<mpsc::UnboundedReceiver<Task>>>,
}

impl SwarmManager {
    pub fn new() -> Self {
        let (task_tx, task_rx) = mpsc::unbounded_channel();
        
        Self {
            tasks: Arc::new(RwLock::new(Vec::new())),
            workers: Arc::new(RwLock::new(Vec::new())),
            task_tx,
            task_rx: Arc::new(RwLock::new(task_rx)),
        }
    }

    /// Register a worker
    pub async fn register_worker(&self, model: &str) -> String {
        let worker = Worker {
            id: Uuid::new_v4().to_string(),
            model: model.to_string(),
            status: WorkerStatus::Idle,
            current_task: None,
            completed_tasks: 0,
            failed_tasks: 0,
            created_at: chrono::Utc::now(),
        };
        
        let worker_id = worker.id.clone();
        self.workers.write().await.push(worker);
        worker_id
    }

    /// Enqueue a task
    pub async fn enqueue_task(&self, goal: &str, priority: u8) -> String {
        let task = Task {
            id: Uuid::new_v4().to_string(),
            goal: goal.to_string(),
            priority,
            status: TaskStatus::Pending,
            assigned_to: None,
            result: None,
            error: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        
        let task_id = task.id.clone();
        self.tasks.write().await.push(task.clone());
        let _ = self.task_tx.send(task);
        task_id
    }

    /// Get next available task (for workers)
    pub async fn next_task(&self, worker_id: &str) -> Option<Task> {
        let mut tasks = self.tasks.write().await;
        
        // Find highest priority pending task
        let mut best_idx = None;
        let mut best_priority = 0;
        
        for (idx, task) in tasks.iter().enumerate() {
            if task.status == TaskStatus::Pending && task.priority > best_priority {
                best_priority = task.priority;
                best_idx = Some(idx);
            }
        }
        
        best_idx.map(|idx| {
            let mut task = tasks[idx].clone();
            task.status = TaskStatus::Assigned;
            task.assigned_to = Some(worker_id.to_string());
            task.updated_at = chrono::Utc::now();
            tasks[idx] = task.clone();
            task
        })
    }

    /// Mark task as completed
    pub async fn complete_task(&self, task_id: &str, result: String) -> anyhow::Result<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = TaskStatus::Completed;
            task.result = Some(result);
            task.updated_at = chrono::Utc::now();
            
            // Update worker stats
            if let Some(worker_id) = &task.assigned_to {
                let mut workers = self.workers.write().await;
                if let Some(worker) = workers.iter_mut().find(|w| w.id == *worker_id) {
                    worker.completed_tasks += 1;
                    worker.status = WorkerStatus::Idle;
                    worker.current_task = None;
                }
            }
        }
        Ok(())
    }

    /// Mark task as failed
    pub async fn fail_task(&self, task_id: &str, error: String) -> anyhow::Result<()> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = TaskStatus::Failed;
            task.error = Some(error);
            task.updated_at = chrono::Utc::now();
            
            // Update worker stats
            if let Some(worker_id) = &task.assigned_to {
                let mut workers = self.workers.write().await;
                if let Some(worker) = workers.iter_mut().find(|w| w.id == *worker_id) {
                    worker.failed_tasks += 1;
                    worker.status = WorkerStatus::Error;
                    worker.current_task = None;
                }
            }
        }
        Ok(())
    }

    /// List all tasks
    pub async fn list_tasks(&self) -> Vec<Task> {
        self.tasks.read().await.clone()
    }

    /// List all workers
    pub async fn list_workers(&self) -> Vec<Worker> {
        self.workers.read().await.clone()
    }

    /// Get task by ID
    pub async fn get_task(&self, task_id: &str) -> Option<Task> {
        self.tasks.read().await.iter().find(|t| t.id == task_id).cloned()
    }

    /// Get worker by ID
    pub async fn get_worker(&self, worker_id: &str) -> Option<Worker> {
        self.workers.read().await.iter().find(|w| w.id == *worker_id).cloned()
    }

    /// Get swarm status
    pub async fn status(&self) -> SwarmStatus {
        let tasks = self.tasks.read().await;
        let workers = self.workers.read().await;
        
        SwarmStatus {
            total_workers: workers.len(),
            idle_workers: workers.iter().filter(|w| w.status == WorkerStatus::Idle).count(),
            total_tasks: tasks.len(),
            pending_tasks: tasks.iter().filter(|t| t.status == TaskStatus::Pending).count(),
            running_tasks: tasks.iter().filter(|t| t.status == TaskStatus::Running).count(),
            completed_tasks: tasks.iter().filter(|t| t.status == TaskStatus::Completed).count(),
            failed_tasks: tasks.iter().filter(|t| t.status == TaskStatus::Failed).count(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SwarmStatus {
    pub total_workers: usize,
    pub idle_workers: usize,
    pub total_tasks: usize,
    pub pending_tasks: usize,
    pub running_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_swarm_workflow() {
        let swarm = SwarmManager::new();
        
        // Register workers
        let w1 = swarm.register_worker("claude-opus-4-6").await;
        let w2 = swarm.register_worker("gemini-2.0").await;
        
        // Enqueue tasks
        let t1 = swarm.enqueue_task("Implement WebSocket", 9).await;
        let t2 = swarm.enqueue_task("Write tests", 5).await;
        
        // Assign tasks
        let task1 = swarm.next_task(&w1).await.unwrap();
        assert_eq!(task1.id, t1);
        
        let task2 = swarm.next_task(&w2).await.unwrap();
        assert_eq!(task2.id, t2);
        
        // Complete tasks
        swarm.complete_task(&t1, "WebSocket implemented!".to_string()).await.unwrap();
        swarm.complete_task(&t2, "Tests written".to_string()).await.unwrap();
        
        // Check status
        let status = swarm.status().await;
        assert_eq!(status.completed_tasks, 2);
        assert_eq!(status.pending_tasks, 0);
    }
}
