//! HTTP/WebSocket gateway for aclaw
//! Allows external tools and UIs to connect to agents

use axum::{
    extract::{ws::{WebSocket, WebSocketUpgrade}, Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Gateway {
    agents: Arc<RwLock<std::collections::HashMap<String, String>>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatRequest {
    pub text: String,
    pub context: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub text: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerStatus {
    pub id: String,
    pub status: String,
    pub memory_mb: u64,
    pub cpu_percent: f32,
}

impl Gateway {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn register_agent(&self, id: String) {
        let mut agents = self.agents.write().await;
        agents.insert(id, String::new());
    }

    pub fn router(&self) -> Router {
        Router::new()
            // Chat endpoints
            .route("/api/chat", post(Self::handle_chat))
            .route("/api/chat/:agent_id", post(Self::handle_chat_agent))
            .route("/ws", get(Self::handle_websocket))
            .route("/ws/:agent_id", get(Self::handle_websocket_agent))
            // Status endpoints
            .route("/api/status", get(Self::handle_status))
            .route("/api/status/:agent_id", get(Self::handle_agent_status))
            .route("/api/containers", get(Self::handle_containers))
            // Memory endpoints
            .route("/api/memory/:namespace", get(Self::handle_memory_list))
            .route("/api/memory/:namespace/:key", get(Self::handle_memory_get))
            // Tool endpoints
            .route("/api/tools", get(Self::handle_tools))
            .route("/api/tools/:tool_name/execute", post(Self::handle_tool_execute))
            // Swarm endpoints
            .route("/api/swarm/tasks", get(Self::handle_swarm_tasks))
            .route("/api/swarm/tasks", post(Self::handle_swarm_enqueue))
            .route("/api/swarm/tasks/:task_id", get(Self::handle_swarm_task_status))
            .route("/api/swarm/workers", get(Self::handle_swarm_workers))
            .route("/api/swarm/status", get(Self::handle_swarm_status))
            // Plugin endpoints
            .route("/api/plugins", get(Self::handle_plugins_list))
            .route("/api/plugins/:plugin_name", get(Self::handle_plugin_info))
            .route("/api/plugins/:plugin_name/call/:method", post(Self::handle_plugin_call))
            .with_state(self.clone())
    }

    async fn handle_chat(
        _state: State<Gateway>,
        _json: Json<ChatRequest>,
    ) -> (StatusCode, Json<ChatResponse>) {
        let response = ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            text: "Chat endpoint: provide agent_id in path".to_string(),
            metadata: serde_json::json!({}),
        };
        (StatusCode::BAD_REQUEST, Json(response))
    }

    async fn handle_chat_agent(
        State(_gateway): State<Gateway>,
        Path(agent_id): Path<String>,
        _json: Json<ChatRequest>,
    ) -> (StatusCode, Json<ChatResponse>) {
        let response = ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            text: format!("Chat to agent {}: (message)", agent_id),
            metadata: serde_json::json!({
                "agent_id": agent_id,
            }),
        };
        (StatusCode::OK, Json(response))
    }

    async fn handle_websocket(ws: WebSocketUpgrade) -> impl IntoResponse {
        ws.on_upgrade(Self::websocket_handler)
    }

    async fn handle_websocket_agent(
        State(_gateway): State<Gateway>,
        Path(agent_id): Path<String>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(|socket| Self::websocket_handler_agent(agent_id, socket))
    }

    async fn websocket_handler(socket: WebSocket) {
        let (_sender, _receiver) = socket.split();
        // WebSocket connection established
    }

    async fn websocket_handler_agent(
        _agent_id: String,
        socket: WebSocket,
    ) {
        let (_sender, _receiver) = socket.split();
        // WebSocket connection established for agent
    }

    async fn handle_status(_state: State<Gateway>) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "agents_connected": 0,
            "uptime_secs": 0,
        }))
    }

    async fn handle_agent_status(
        State(_gateway): State<Gateway>,
        Path(agent_id): Path<String>,
    ) -> (StatusCode, Json<ContainerStatus>) {
        let status = ContainerStatus {
            id: agent_id,
            status: "running".to_string(),
            memory_mb: 5,
            cpu_percent: 0.1,
        };
        (StatusCode::OK, Json(status))
    }

    async fn handle_containers(
        State(_gateway): State<Gateway>,
    ) -> Json<Vec<ContainerStatus>> {
        Json(vec![])
    }

    async fn handle_memory_list(
        State(_gateway): State<Gateway>,
        Path(_namespace): Path<String>,
    ) -> Json<Vec<String>> {
        Json(vec![])
    }

    async fn handle_memory_get(
        State(_gateway): State<Gateway>,
        Path((namespace, key)): Path<(String, String)>,
    ) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "namespace": namespace,
            "key": key,
            "value": "example_value"
        }))
    }

    async fn handle_tools(
        State(_gateway): State<Gateway>,
    ) -> Json<Vec<serde_json::Value>> {
        Json(vec![
            serde_json::json!({
                "name": "shell",
                "description": "Execute shell commands"
            }),
            serde_json::json!({
                "name": "file_read",
                "description": "Read files (50KB limit, path safe)"
            }),
        ])
    }

    async fn handle_tool_execute(
        State(_gateway): State<Gateway>,
        Path(tool_name): Path<String>,
        _json: Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "tool": tool_name,
                "output": "Tool execution result"
            })),
        )
    }

    // Swarm endpoints
    async fn handle_swarm_tasks(
        State(_gateway): State<Gateway>,
    ) -> Json<Vec<serde_json::Value>> {
        Json(vec![])
    }

    async fn handle_swarm_enqueue(
        State(_gateway): State<Gateway>,
        Json(payload): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        let goal = payload.get("goal").and_then(|v| v.as_str()).unwrap_or("untitled");
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "task_id": uuid::Uuid::new_v4().to_string(),
                "goal": goal,
                "status": "pending"
            })),
        )
    }

    async fn handle_swarm_task_status(
        State(_gateway): State<Gateway>,
        Path(task_id): Path<String>,
    ) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "task_id": task_id,
            "status": "pending",
            "progress": 0
        }))
    }

    async fn handle_swarm_workers(
        State(_gateway): State<Gateway>,
    ) -> Json<Vec<serde_json::Value>> {
        Json(vec![])
    }

    async fn handle_swarm_status(
        State(_gateway): State<Gateway>,
    ) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "total_workers": 0,
            "idle_workers": 0,
            "total_tasks": 0,
            "pending_tasks": 0,
            "completed_tasks": 0
        }))
    }

    // Plugin endpoints
    async fn handle_plugins_list(
        State(_gateway): State<Gateway>,
    ) -> Json<Vec<String>> {
        Json(vec!["ai".to_string(), "tools".to_string(), "vibemania".to_string(), "git".to_string()])
    }

    async fn handle_plugin_info(
        State(_gateway): State<Gateway>,
        Path(plugin_name): Path<String>,
    ) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "name": plugin_name,
            "version": "0.1.0",
            "methods": []
        }))
    }

    async fn handle_plugin_call(
        State(_gateway): State<Gateway>,
        Path((plugin_name, method)): Path<(String, String)>,
        Json(params): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "plugin": plugin_name,
                "method": method,
                "params": params,
                "result": "Plugin call result"
            })),
        )
    }
}

pub async fn start_gateway(addr: &str) -> anyhow::Result<()> {
    let gateway = Gateway::new();
    let app = gateway.router();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Gateway listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
