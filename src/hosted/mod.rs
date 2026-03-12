//! Hosted control-plane runtime for multi-user gateway deployments.
//! Stores tenant/session metadata in SurrealDB + RocksDB and
//! keeps one live AgentRunner per active session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::engine::local::RocksDb;
use surrealdb::Surreal;
use tokio::sync::RwLock;

use crate::agent::AgentRunner;
use crate::channels::IncomingMessage;
use crate::config::Config;
use crate::memory::{
    search::{MemoryGetTool, MemorySearchTool},
    traits::MemoryEntry,
    MemoryBackend,
};
use crate::plugin::{
    AiPlugin, GitPlugin, PluginInfo, PluginRegistry, ToolsPlugin, VibemaniaPlugin,
};
use crate::policy::ExecutionPolicy;
use crate::providers::Provider;
use crate::skills;
use crate::swarm::{AgentInfo, SurrealBackend, SwarmCoordinator, SwarmStorage, Task, TaskPriority};
use crate::tools::file_ops::{FileReadTool, FileWriteTool};
use crate::tools::shell::ShellTool;
use crate::tools::{Tool, ToolResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantRecord {
    pub tenant_id: String,
    pub user_id: String,
    pub default_channel: String,
    pub workspace: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub tenant_id: String,
    pub agent_key: String,
    pub channel: String,
    pub model: String,
    pub workspace: String,
    pub runtime_kind: String,
    pub status: String,
    pub last_active: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLease {
    pub tenant: TenantRecord,
    pub session: SessionRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedStatus {
    pub sessions: usize,
    pub runtime_kind: String,
    pub queue_depth: usize,
    pub tenant_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantHealth {
    pub tenant_id: String,
    pub sessions: usize,
    pub total_requests: u64,
    pub total_errors: u64,
    pub average_latency_ms: u64,
    pub last_seen: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RuntimeMetrics {
    pub total_requests: u64,
    pub total_errors: u64,
    pub auth_failures: u64,
    pub rate_limited: u64,
    pub average_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInstanceStatus {
    pub session_id: String,
    pub tenant_id: String,
    pub model: String,
    pub workspace: String,
    pub runtime_kind: String,
    pub cost_total_usd: f64,
    pub total_tokens: usize,
    pub call_count: usize,
}

#[derive(Debug, Default, Clone)]
struct TenantMetrics {
    total_requests: u64,
    total_errors: u64,
    total_latency_ms: u64,
    last_seen: Option<DateTime<Utc>>,
}

pub struct HostedControlPlane {
    db: Surreal<surrealdb::engine::local::Db>,
}

impl HostedControlPlane {
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = Surreal::new::<RocksDb>(path.as_ref()).await?;
        db.use_ns("claw").use_db("control").await?;
        db.query(SCHEMA_SQL).await?;
        Ok(Self { db })
    }

    pub async fn ensure_session(
        &self,
        user_id: &str,
        session_id: &str,
        channel: &str,
        workspace: &Path,
        model: &str,
        runtime_kind: &str,
        agent_key: &str,
    ) -> Result<SessionLease> {
        let now = Utc::now();
        let tenant_id = format!("tenant:{}", sanitize_key(user_id));
        let tenant = TenantRecord {
            tenant_id: tenant_id.clone(),
            user_id: user_id.to_string(),
            default_channel: channel.to_string(),
            workspace: workspace.display().to_string(),
            created_at: now,
            updated_at: now,
        };
        let session = SessionRecord {
            session_id: session_id.to_string(),
            tenant_id: tenant_id.clone(),
            agent_key: agent_key.to_string(),
            channel: channel.to_string(),
            model: model.to_string(),
            workspace: workspace.display().to_string(),
            runtime_kind: runtime_kind.to_string(),
            status: "active".to_string(),
            last_active: now,
            created_at: now,
        };

        let _: Option<TenantRecord> = self
            .db
            .upsert(("tenants", &tenant_id))
            .content(tenant.clone())
            .await?;
        let _: Option<SessionRecord> = self
            .db
            .upsert(("sessions", session_id))
            .content(session.clone())
            .await?;

        Ok(SessionLease { tenant, session })
    }

    pub async fn touch_session(&self, session_id: &str) -> Result<()> {
        self.db
            .query(
                "UPDATE sessions SET last_active = time::now(), status = 'active' WHERE session_id = $session_id",
            )
            .bind(("session_id", session_id.to_string()))
            .await?;
        Ok(())
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>> {
        let mut result = self
            .db
            .query("SELECT * FROM sessions ORDER BY last_active DESC")
            .await?;
        Ok(result.take(0)?)
    }
}

const SCHEMA_SQL: &str = r#"
    DEFINE TABLE IF NOT EXISTS tenants SCHEMALESS;
    DEFINE FIELD IF NOT EXISTS tenant_id ON tenants TYPE string;
    DEFINE FIELD IF NOT EXISTS user_id ON tenants TYPE string;
    DEFINE FIELD IF NOT EXISTS default_channel ON tenants TYPE string;
    DEFINE FIELD IF NOT EXISTS workspace ON tenants TYPE string;
    DEFINE FIELD IF NOT EXISTS created_at ON tenants TYPE datetime;
    DEFINE FIELD IF NOT EXISTS updated_at ON tenants TYPE datetime;
    DEFINE INDEX IF NOT EXISTS tenant_id_idx ON tenants FIELDS tenant_id UNIQUE;
    DEFINE INDEX IF NOT EXISTS tenant_user_idx ON tenants FIELDS user_id UNIQUE;

    DEFINE TABLE IF NOT EXISTS sessions SCHEMALESS;
    DEFINE FIELD IF NOT EXISTS session_id ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS tenant_id ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS agent_key ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS channel ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS model ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS workspace ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS runtime_kind ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS status ON sessions TYPE string;
    DEFINE FIELD IF NOT EXISTS last_active ON sessions TYPE datetime;
    DEFINE FIELD IF NOT EXISTS created_at ON sessions TYPE datetime;
    DEFINE INDEX IF NOT EXISTS session_id_idx ON sessions FIELDS session_id UNIQUE;
"#;

pub struct HostedRuntime {
    config: Config,
    workspace: PathBuf,
    provider: Arc<dyn Provider>,
    memory: Arc<dyn MemoryBackend>,
    policy: Arc<ExecutionPolicy>,
    system_prompt: String,
    skills: Vec<skills::Skill>,
    control: Arc<HostedControlPlane>,
    sessions: Arc<RwLock<HashMap<String, Arc<AgentRunner>>>>,
    swarm: Arc<SwarmCoordinator>,
    plugins: Arc<PluginRegistry>,
    metrics: Arc<RwLock<RuntimeMetrics>>,
    tenant_metrics: Arc<RwLock<HashMap<String, TenantMetrics>>>,
}

impl HostedRuntime {
    pub async fn new(
        config: Config,
        workspace: PathBuf,
        provider: Arc<dyn Provider>,
        memory: Arc<dyn MemoryBackend>,
        policy: Arc<ExecutionPolicy>,
        system_prompt: String,
        skills: Vec<skills::Skill>,
        state_path: PathBuf,
    ) -> Result<Self> {
        let control = HostedControlPlane::new(&state_path)
            .await
            .context("failed to initialize hosted control plane")?;
        let swarm_storage: Arc<dyn SwarmStorage> = Arc::new(
            SurrealBackend::new(&state_path)
                .await
                .context("failed to initialize swarm storage")?,
        );
        let swarm = Arc::new(SwarmCoordinator::new(swarm_storage));
        swarm.init().await?;

        let mut plugins = PluginRegistry::new();
        plugins.register(Arc::new(AiPlugin));
        plugins.register(Arc::new(ToolsPlugin::new(policy.clone())));
        plugins.register(Arc::new(VibemaniaPlugin));
        plugins.register(Arc::new(GitPlugin::new(policy.clone())));

        Ok(Self {
            config,
            workspace,
            provider,
            memory,
            policy,
            system_prompt,
            skills,
            control: Arc::new(control),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            swarm,
            plugins: Arc::new(plugins),
            metrics: Arc::new(RwLock::new(RuntimeMetrics::default())),
            tenant_metrics: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn chat(
        &self,
        text: &str,
        user_id: Option<&str>,
        session_id: Option<&str>,
        channel: Option<&str>,
        agent_key: Option<&str>,
    ) -> Result<(String, SessionLease)> {
        let resolved_user = user_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("gateway-user");
        let resolved_channel = channel
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.config.hosting.default_channel);
        let resolved_session = session_id
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    sanitize_key(resolved_channel),
                    sanitize_key(resolved_user)
                )
            });
        let resolved_agent = agent_key
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("main");
        let session_workspace = self.session_workspace(resolved_channel, resolved_user);
        tokio::fs::create_dir_all(&session_workspace).await?;

        let lease = self
            .control
            .ensure_session(
                resolved_user,
                &resolved_session,
                resolved_channel,
                &session_workspace,
                &self.config.model,
                &self.config.runtime.kind,
                resolved_agent,
            )
            .await?;

        let started_at = std::time::Instant::now();
        let runner = self.get_or_create_runner(&lease).await?;
        let outcome = runner
            .handle_message_pub(
                &IncomingMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    sender_id: resolved_user.to_string(),
                    sender_name: None,
                    chat_id: resolved_session.clone(),
                    text: text.to_string(),
                    is_group: false,
                    reply_to: None,
                    timestamp: Utc::now(),
                },
                None,
            )
            .await;
        let latency_ms = started_at.elapsed().as_millis() as u64;
        self.control.touch_session(&resolved_session).await?;

        match outcome {
            Ok(response) => {
                self.record_chat_metrics(&lease.tenant.tenant_id, latency_ms, false)
                    .await;
                Ok((response, lease))
            }
            Err(error) => {
                self.record_chat_metrics(&lease.tenant.tenant_id, latency_ms, true)
                    .await;
                Err(error)
            }
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionRecord>> {
        self.control.list_sessions().await
    }

    pub async fn status(&self) -> Result<HostedStatus> {
        let sessions = self.list_sessions().await?;
        let tenant_count = self.tenant_metrics.read().await.len();
        Ok(HostedStatus {
            sessions: sessions.len(),
            runtime_kind: self.config.runtime.kind.clone(),
            queue_depth: self.sessions.read().await.len(),
            tenant_count,
        })
    }

    pub async fn list_memory(&self, namespace: &str) -> Result<Vec<MemoryEntry>> {
        self.memory.list(namespace).await
    }

    pub async fn get_memory(&self, namespace: &str, key: &str) -> Result<Option<MemoryEntry>> {
        self.memory.recall(namespace, key).await
    }

    pub fn safe_gateway_tools(&self) -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(crate::tools::doctor::DoctorTool::new()),
            Arc::new(crate::tools::session::ListModelsTool::new()),
        ]
    }

    pub async fn execute_gateway_tool(
        &self,
        tool_name: &str,
        arguments: &str,
    ) -> Result<ToolResult> {
        let Some(tool) = self
            .safe_gateway_tools()
            .into_iter()
            .find(|candidate| candidate.name().eq_ignore_ascii_case(tool_name))
        else {
            anyhow::bail!("tool '{tool_name}' is not allowed over the gateway");
        };
        tool.execute(arguments).await
    }

    pub fn list_plugins(&self) -> Vec<String> {
        self.plugins.list()
    }

    pub fn plugin_info(&self, name: &str) -> Option<PluginInfo> {
        self.plugins.info(name)
    }

    pub async fn call_plugin(
        &self,
        plugin_name: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.plugins
            .call(plugin_name, method, params)
            .await
            .map_err(|error| anyhow::anyhow!(error.message))
    }

    pub async fn list_swarm_tasks(&self) -> Result<Vec<Task>> {
        self.swarm.list_pending_tasks().await
    }

    pub async fn enqueue_swarm_task(&self, goal: &str) -> Result<String> {
        let title = goal.lines().next().unwrap_or(goal).trim();
        self.swarm
            .submit_task(title.to_string(), goal.to_string(), TaskPriority::Medium)
            .await
    }

    pub async fn list_swarm_agents(&self) -> Result<Vec<AgentInfo>> {
        self.swarm.list_all_agents().await
    }

    pub async fn get_swarm_task(&self, task_id: &str) -> Result<Option<Task>> {
        self.swarm.storage().get_task(task_id).await
    }

    pub async fn metrics(&self) -> RuntimeMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn tenant_health(&self) -> Vec<TenantHealth> {
        let sessions = self.list_sessions().await.unwrap_or_default();
        let mut sessions_by_tenant = HashMap::<String, usize>::new();
        for session in sessions {
            *sessions_by_tenant.entry(session.tenant_id).or_default() += 1;
        }

        let metrics = self.tenant_metrics.read().await;
        metrics
            .iter()
            .map(|(tenant_id, value)| TenantHealth {
                tenant_id: tenant_id.clone(),
                sessions: sessions_by_tenant.get(tenant_id).copied().unwrap_or(0),
                total_requests: value.total_requests,
                total_errors: value.total_errors,
                average_latency_ms: if value.total_requests == 0 {
                    0
                } else {
                    value.total_latency_ms / value.total_requests
                },
                last_seen: value.last_seen,
            })
            .collect()
    }

    pub async fn runtime_instances(&self) -> Vec<RuntimeInstanceStatus> {
        let session_pairs: Vec<(String, Arc<AgentRunner>)> = {
            let sessions = self.sessions.read().await;
            sessions
                .iter()
                .map(|(session_id, runner)| (session_id.clone(), runner.clone()))
                .collect()
        };
        let stored_sessions = self.list_sessions().await.unwrap_or_default();
        let mut session_map = HashMap::new();
        for session in stored_sessions {
            session_map.insert(session.session_id.clone(), session);
        }

        let mut instances = Vec::new();
        for (session_id, runner) in session_pairs {
            if let Some(session) = session_map.get(&session_id) {
                let cost = runner.get_cost_summary().await;
                instances.push(RuntimeInstanceStatus {
                    session_id,
                    tenant_id: session.tenant_id.clone(),
                    model: session.model.clone(),
                    workspace: session.workspace.clone(),
                    runtime_kind: session.runtime_kind.clone(),
                    cost_total_usd: cost.total_cost,
                    total_tokens: cost.total_tokens,
                    call_count: cost.call_count,
                });
            }
        }
        instances
    }

    pub async fn record_auth_failure(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.auth_failures += 1;
    }

    pub async fn record_rate_limited(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.rate_limited += 1;
    }

    async fn record_chat_metrics(&self, tenant_id: &str, latency_ms: u64, is_error: bool) {
        {
            let mut metrics = self.metrics.write().await;
            metrics.total_requests += 1;
            if is_error {
                metrics.total_errors += 1;
            }
            metrics.average_latency_ms = rolling_average(
                metrics.average_latency_ms,
                metrics.total_requests,
                latency_ms,
            );
        }
        let mut tenants = self.tenant_metrics.write().await;
        let entry = tenants.entry(tenant_id.to_string()).or_default();
        entry.total_requests += 1;
        if is_error {
            entry.total_errors += 1;
        }
        entry.total_latency_ms += latency_ms;
        entry.last_seen = Some(Utc::now());
    }

    async fn get_or_create_runner(&self, lease: &SessionLease) -> Result<Arc<AgentRunner>> {
        let session_id = &lease.session.session_id;
        if let Some(existing) = self.sessions.read().await.get(session_id).cloned() {
            return Ok(existing);
        }

        let session_workspace = PathBuf::from(&lease.session.workspace);
        let runner = Arc::new(
            AgentRunner::new(
                self.provider.clone(),
                build_hosted_tools(&session_workspace, self.policy.clone()),
                self.memory.clone(),
                self.system_prompt.clone(),
                lease.session.model.clone(),
            )
            .with_workspace(session_workspace)
            .with_skills(self.skills.clone())
            .await,
        );
        runner
            .add_tool(Arc::new(crate::tools::session::SessionStatusTool::new(
                runner.clone(),
            )))
            .await;
        runner
            .add_tool(Arc::new(crate::tools::claude_usage::ClaudeUsageTool::new(
                runner.cost_tracker(),
            )))
            .await;

        let mut sessions = self.sessions.write().await;
        let entry = sessions
            .entry(session_id.to_string())
            .or_insert_with(|| runner.clone())
            .clone();
        Ok(entry)
    }

    fn session_workspace(&self, channel: &str, user_id: &str) -> PathBuf {
        self.workspace
            .join(&self.config.hosting.tenant_root)
            .join(sanitize_key(channel))
            .join(sanitize_key(user_id))
            .join("workspace")
    }
}

fn build_hosted_tools(workspace: &Path, policy: Arc<ExecutionPolicy>) -> Vec<Arc<dyn Tool>> {
    let workspace = workspace.to_path_buf();
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(ShellTool::new(workspace.clone(), Arc::clone(&policy))),
        Arc::new(FileReadTool::new(workspace.clone())),
        Arc::new(FileWriteTool::new(workspace.clone())),
        Arc::new(crate::tools::edit::EditTool::new(workspace.clone())),
        Arc::new(MemorySearchTool::new(workspace.clone())),
        Arc::new(MemoryGetTool::new(workspace.clone())),
        Arc::new(crate::tools::web_search::WebSearchTool::new()),
        Arc::new(crate::tools::web_fetch::WebFetchTool::new()),
        Arc::new(crate::tools::doctor::DoctorTool::new()),
        Arc::new(crate::tools::session::ListModelsTool::new()),
        Arc::new(crate::tools::dynamic::CreateToolTool::new(Arc::clone(
            &policy,
        ))),
        Arc::new(crate::tools::dynamic::ListCustomToolsTool::new()),
        Arc::new(crate::tools::browser::BrowserTool::new()),
        Arc::new(crate::tools::mcp::McpTool::new()),
    ];

    for tool in crate::tools::dynamic::DynamicTool::load_all(policy) {
        tools.push(Arc::new(tool));
    }

    tools
}

fn sanitize_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

pub fn default_state_path(workspace: &Path, configured: Option<&PathBuf>) -> PathBuf {
    configured
        .cloned()
        .unwrap_or_else(|| workspace.join(".unthinkclaw/state.surreal"))
}

fn rolling_average(current_avg: u64, total_requests: u64, latest: u64) -> u64 {
    if total_requests <= 1 {
        latest
    } else {
        ((((current_avg as u128) * ((total_requests - 1) as u128)) + (latest as u128))
            / (total_requests as u128)) as u64
    }
}
