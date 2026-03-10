//! Agent loop — the core execution engine.
//! Processes incoming messages, calls LLM, executes tools, sends responses.
//! Supports progress callbacks for real-time feedback.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::RwLock;

use crate::channels::{Channel, IncomingMessage, OutgoingMessage};
use crate::cost::{CostTracker, TokenUsage};
use crate::memory::MemoryBackend;
use crate::providers::{ChatMessage, ChatRequest, Provider};
use crate::skills;
use crate::tools::Tool;

/// Compact conversation after this many rounds (uses Haiku to summarize)
const COMPACTION_THRESHOLD: usize = 15;
/// Hard circuit breaker (absolute max rounds)
const CIRCUIT_BREAKER_ROUNDS: usize = 50;
/// Warn the LLM after this many identical tool calls
const LOOP_WARN_THRESHOLD: usize = 5;
/// Hard stop after this many identical consecutive tool calls
const LOOP_BREAK_THRESHOLD: usize = 8;
/// Max conversation history (prevents context overflow)
const MAX_HISTORY_MESSAGES: usize = 8;
/// Max chars for a single tool result (OpenClaw-style truncation)
const MAX_TOOL_RESULT_CHARS: usize = 20_000;
/// Max context chars before triggering mid-loop compaction
const MAX_CONTEXT_CHARS: usize = 150_000;
/// Haiku model for compaction (fast + cheap)
const COMPACTION_MODEL: &str = "claude-haiku-4-5";

/// Progress update sent during agent processing
#[derive(Debug, Clone)]
pub enum ProgressUpdate {
    /// Agent is thinking (first LLM call)
    Thinking,
    /// Agent is calling a tool
    ToolCall { name: String, round: usize },
    /// Agent got tool result, calling LLM again
    Processing { round: usize, tool_count: usize },
}

pub struct AgentRunner {
    provider: Arc<dyn Provider>,
    /// Hot-reloadable tools list — shared with watcher + create_tool
    pub tools: Arc<RwLock<Vec<Arc<dyn Tool>>>>,
    memory: Arc<dyn MemoryBackend>,
    /// Hot-reloadable system prompt — updated when MEMORY.md / context files change
    pub system_prompt: Arc<RwLock<String>>,
    model: std::sync::RwLock<String>,
    workspace: PathBuf,
    /// Hot-reloadable skills — re-discovered when skills/ dir changes
    pub skills: Arc<RwLock<Vec<skills::Skill>>>,
    cost_tracker: Arc<CostTracker>,
    /// Steering messages — injected into the loop between rounds
    pub steering_queue: Arc<std::sync::Mutex<Vec<String>>>,
}

impl AgentRunner {
    pub fn new(
        provider: Arc<dyn Provider>,
        tools: Vec<Arc<dyn Tool>>,
        memory: Arc<dyn MemoryBackend>,
        system_prompt: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            tools: Arc::new(RwLock::new(tools)),
            memory,
            system_prompt: Arc::new(RwLock::new(system_prompt.into())),
            model: std::sync::RwLock::new(model.into()),
            workspace: PathBuf::from("."),
            skills: Arc::new(RwLock::new(Vec::new())),
            cost_tracker: Arc::new(CostTracker::new()),
            steering_queue: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Queue a steering message to inject into the current agent loop
    pub fn steer(&self, message: String) {
        self.steering_queue.lock().unwrap().push(message);
    }

    pub fn with_workspace(mut self, workspace: PathBuf) -> Self {
        self.workspace = workspace;
        self
    }

    pub async fn with_skills(self, skills: Vec<skills::Skill>) -> Self {
        *self.skills.write().await = skills;
        self
    }

    /// Get cost tracker reference (for ClaudeUsageTool)
    pub fn cost_tracker(&self) -> Arc<CostTracker> {
        self.cost_tracker.clone()
    }

    /// Get cost summary
    pub async fn get_cost_summary(&self) -> crate::cost::CostSummary {
        self.cost_tracker.summary().await
    }

    /// Get current model name
    pub fn get_model(&self) -> String {
        self.model.read().unwrap().clone()
    }

    /// Switch model at runtime
    pub fn set_model(&self, model: impl Into<String>) {
        *self.model.write().unwrap() = model.into();
    }

    /// List available tools
    pub async fn list_tools(&self) -> Vec<String> {
        self.tools.read().await.iter().map(|t| t.name().to_string()).collect()
    }

    /// Add a tool at runtime (for late-binding tools like session_status)
    pub async fn add_tool(&self, tool: Arc<dyn Tool>) {
        self.tools.write().await.push(tool);
    }

    /// Run the agent loop on a channel.
    pub async fn run(&self, channel: &mut dyn Channel) -> anyhow::Result<()> {
        let mut rx = channel.start().await?;
        tracing::info!("Agent started on channel: {}", channel.name());

        while let Some(msg) = rx.recv().await {
            // Send typing indicator
            let progress_tx = self.setup_progress(channel).await;

            match self.handle_message(&msg, Some(&progress_tx)).await {
                Ok(response) => {
                    // Signal done to progress tracker
                    let _ = progress_tx.send(ProgressUpdate::Processing { round: 0, tool_count: 0 }).await;

                    channel.send(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: response,
                        reply_to: Some(msg.id.clone()),
                    }).await?;
                }
                Err(e) => {
                    tracing::error!("Error handling message: {}", e);
                    channel.send(OutgoingMessage {
                        chat_id: msg.chat_id,
                        text: format!("Error: {}", e),
                        reply_to: Some(msg.id),
                    }).await?;
                }
            }
        }

        channel.stop().await?;
        Ok(())
    }

    /// Run the agent loop with an additional message source (e.g., heartbeat).
    pub async fn run_with_extra_rx(
        &self,
        channel: &mut dyn Channel,
        mut extra_rx: mpsc::Receiver<IncomingMessage>,
    ) -> anyhow::Result<()> {
        let mut rx = channel.start().await?;
        tracing::info!("Agent started on channel: {} (with heartbeat)", channel.name());

        loop {
            let msg = tokio::select! {
                Some(msg) = rx.recv() => msg,
                Some(msg) = extra_rx.recv() => msg,
                else => break,
            };

            let progress_tx = self.setup_progress(channel).await;

            match self.handle_message(&msg, Some(&progress_tx)).await {
                Ok(response) => {
                    if msg.sender_id == "system" && response.contains("HEARTBEAT_OK") {
                        tracing::debug!("Heartbeat: agent responded OK, skipping output");
                        continue;
                    }
                    channel.send(OutgoingMessage {
                        chat_id: msg.chat_id.clone(),
                        text: response,
                        reply_to: Some(msg.id.clone()),
                    }).await?;
                }
                Err(e) => {
                    tracing::error!("Error handling message: {}", e);
                    if msg.sender_id != "system" {
                        channel.send(OutgoingMessage {
                            chat_id: msg.chat_id,
                            text: format!("Error: {}", e),
                            reply_to: Some(msg.id),
                        }).await?;
                    }
                }
            }
        }

        channel.stop().await?;
        Ok(())
    }

    async fn setup_progress(&self, _channel: &dyn Channel) -> mpsc::Sender<ProgressUpdate> {
        let (tx, mut rx) = mpsc::channel(32);
        
        // Clone channel for the progress task
        // Note: This requires Channel to be Clone or use Arc
        // For now, we'll skip the actual typing indicator until Channel is made Clone-safe
        tokio::spawn(async move {
            while let Some(_update) = rx.recv().await {
                // TODO: Send typing indicator via channel
                // For now, just drain the channel
            }
        });
        
        tx
    }

    /// Public handle message (for custom channel loops like Telegram with progress)
    pub async fn handle_message_pub(
        &self,
        msg: &IncomingMessage,
        progress: Option<&mpsc::Sender<ProgressUpdate>>,
    ) -> anyhow::Result<String> {
        self.handle_message(msg, progress).await
    }

    /// Handle a single message — LLM call with tool loop + conversation history.
    async fn handle_message(
        &self,
        msg: &IncomingMessage,
        progress: Option<&mpsc::Sender<ProgressUpdate>>,
    ) -> anyhow::Result<String> {
        // Signal thinking
        if let Some(tx) = progress {
            let _ = tx.send(ProgressUpdate::Thinking).await;
        }

        // Build messages: system prompt + conversation history + new message
        let system_prompt = self.system_prompt.read().await.clone();
        let mut messages = vec![ChatMessage::system(&system_prompt)];

        // Skill injection
        {
            let skills = self.skills.read().await;
            if let Some(skill) = skills::match_skill(&skills, &msg.text) {
                if let Some(content) = skills::load_skill_content(skill) {
                    messages.push(ChatMessage::system(format!(
                        "# Active Skill: {}\n{}\n\nFollow the instructions above for this skill.",
                        skill.name, content
                    )));
                    tracing::info!("Skill matched: {}", skill.name);
                }
            }
        }

        // Load conversation history from SQLite (LIMITED to 8 to save tokens)
        let history = self.memory.get_conversation_history(&msg.chat_id, MAX_HISTORY_MESSAGES).await?;
        for (role, content) in history {
            match role.as_str() {
                "user" => messages.push(ChatMessage::user(&content)),
                "assistant" => messages.push(ChatMessage::assistant(&content)),
                _ => {} // Skip unknown roles
            }
        }

        // Add new user message
        messages.push(ChatMessage::user(&msg.text));

        // Tool specs — snapshot at message start (hot-reload takes effect next message)
        let tool_specs: Vec<crate::tools::ToolSpec> = self.tools.read().await.iter()
            .map(|t| t.spec())
            .collect();
        let tools_snapshot: Vec<Arc<dyn Tool>> = self.tools.read().await.iter().cloned().collect();

        // Agent loop with compaction
        let mut tool_call_history: Vec<String> = Vec::new();
        let mut compactions_done: usize = 0;

        for round in 0..CIRCUIT_BREAKER_ROUNDS {
            // Check for steering messages
            {
                let mut queue = self.steering_queue.lock().unwrap();
                if !queue.is_empty() {
                    for steer_msg in queue.drain(..) {
                        tracing::info!("Steering message injected: {}", &steer_msg[..steer_msg.len().min(80)]);
                        messages.push(ChatMessage::user(format!(
                            "⚡ STEERING — new instruction from user (prioritize this): {}", steer_msg
                        )));
                    }
                }
            }

            // Mid-loop compaction: if context is too large or we hit the threshold
            let context_chars: usize = messages.iter().map(|m| m.content.len()).sum();
            if (round > 0 && round % COMPACTION_THRESHOLD == 0) || context_chars > MAX_CONTEXT_CHARS {
                tracing::info!("Compacting conversation at round {} ({} chars, {} msgs)", 
                    round + 1, context_chars, messages.len());
                messages = self.compact_messages(messages, &msg.text).await?;
                compactions_done += 1;
                tracing::info!("Compacted to {} msgs, {} chars", 
                    messages.len(), 
                    messages.iter().map(|m| m.content.len()).sum::<usize>());
            }

            tracing::info!("Agent round {} — {} messages, ~{} chars", 
                round + 1, messages.len(),
                messages.iter().map(|m| m.content.len()).sum::<usize>());

            let request = ChatRequest {
                messages: messages.clone(),
                tools: if tool_specs.is_empty() { None } else { Some(tool_specs.clone()) },
                model: self.model.read().unwrap().clone(),
                temperature: 0.3, // Low temp for reliable tool-calling
                max_tokens: Some(8192),
            };

            let response = self.provider.chat(&request).await?;

            // Track cost if usage is available
            if let Some(usage) = &response.usage {
                let _ = self.cost_tracker.record(
                    &self.model.read().unwrap(),
                    TokenUsage {
                        input_tokens: usage.input_tokens as usize,
                        output_tokens: usage.output_tokens as usize,
                        total_tokens: (usage.input_tokens + usage.output_tokens) as usize,
                    }
                ).await;
            }

            if !response.has_tool_calls() {
                // Done — return text and persist to history
                tracing::info!("Agent done after {} round(s)", round + 1);
                let text = response.text.unwrap_or_default();

                // Store user message to SQLite
                self.memory.store_conversation(
                    &msg.chat_id,
                    &msg.sender_id,
                    "user",
                    &msg.text,
                ).await?;

                // Store assistant response to SQLite
                self.memory.store_conversation(
                    &msg.chat_id,
                    "assistant",
                    "assistant",
                    &text,
                ).await?;

                return Ok(text);
            }

            // Loop detection
            for tc in &response.tool_calls {
                let hash = format!("{}:{}", tc.name, tc.arguments);
                tool_call_history.push(hash);
            }
            if tool_call_history.len() >= LOOP_BREAK_THRESHOLD {
                let last = &tool_call_history[tool_call_history.len() - 1];
                let consecutive = tool_call_history.iter().rev().take_while(|h| *h == last).count();
                if consecutive >= LOOP_BREAK_THRESHOLD {
                    tracing::warn!("Loop detected: {} identical calls, breaking", consecutive);
                    return Ok(format!("Got stuck in a loop calling {} {} times. Try rephrasing?",
                        response.tool_calls[0].name, consecutive));
                }
                if consecutive >= LOOP_WARN_THRESHOLD {
                    messages.push(ChatMessage::user(format!(
                        "WARNING: You called {} {} times identically. Stop retrying and answer with what you have.",
                        response.tool_calls[0].name, consecutive
                    )));
                }
            }

            // Progress callback
            if let Some(tx) = progress {
                for tc in &response.tool_calls {
                    let _ = tx.send(ProgressUpdate::ToolCall {
                        name: tc.name.clone(),
                        round: round + 1,
                    }).await;
                }
            }

            // Build assistant message with tool_use content blocks
            {
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();
                if let Some(text) = &response.text {
                    if !text.is_empty() {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }
                for tc in &response.tool_calls {
                    content_blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": &tc.id,
                        "name": &tc.name,
                        "input": serde_json::from_str::<serde_json::Value>(&tc.arguments).unwrap_or_default(),
                    }));
                }
                messages.push(ChatMessage {
                    role: "assistant_tool_use".to_string(),
                    content: String::new(),
                    tool_use_id: Some(serde_json::to_string(&content_blocks).unwrap_or_default()),
                });
            }

            // Execute each tool call
            tracing::info!("Tool calls: {}",
                response.tool_calls.iter().map(|tc| tc.name.as_str()).collect::<Vec<_>>().join(", "));

            for tc in &response.tool_calls {
                let result = if let Some(tool) = tools_snapshot.iter().find(|t| t.name() == tc.name) {
                    match tool.execute(&tc.arguments).await {
                        Ok(r) => r,
                        Err(e) => crate::tools::ToolResult::error(format!("Tool error: {}", e)),
                    }
                } else {
                    crate::tools::ToolResult::error(format!("Unknown tool: {}", tc.name))
                };

                // TRUNCATE tool result to save tokens (OpenClaw-style)
                let truncated_output = if result.output.len() > MAX_TOOL_RESULT_CHARS {
                    format!(
                        "{}...\n⚠️ [Truncated from {} to {} chars. Use offset/limit params.]",
                        &result.output[..MAX_TOOL_RESULT_CHARS],
                        result.output.len(),
                        MAX_TOOL_RESULT_CHARS
                    )
                } else {
                    result.output.clone()
                };

                messages.push(ChatMessage::tool_result(&tc.id, &truncated_output));
            }
        }

        tracing::warn!("Circuit breaker after {} rounds ({} compactions)", CIRCUIT_BREAKER_ROUNDS, compactions_done);
        
        self.memory.store_conversation(&msg.chat_id, &msg.sender_id, "user", &msg.text).await?;
        self.memory.store_conversation(
            &msg.chat_id, "assistant", "assistant",
            &format!("Hit {} rounds ({} compactions). Task may need to be broken down.", 
                CIRCUIT_BREAKER_ROUNDS, compactions_done),
        ).await?;
        
        Ok(format!(
            "⚠️ Hit {} rounds ({} compactions performed). Break this into smaller tasks?",
            CIRCUIT_BREAKER_ROUNDS, compactions_done
        ))
    }

    /// Compact conversation using Haiku — summarize old messages, keep recent ones
    async fn compact_messages(
        &self,
        messages: Vec<ChatMessage>,
        original_task: &str,
    ) -> anyhow::Result<Vec<ChatMessage>> {
        // Keep: system prompt + last 6 messages (3 exchanges)
        let keep_recent = 6;
        
        if messages.len() <= keep_recent + 2 {
            return Ok(messages); // Nothing to compact
        }
        
        // Split: system messages + old messages + recent messages
        let system_msgs: Vec<&ChatMessage> = messages.iter()
            .filter(|m| m.role == "system")
            .collect();
        let non_system: Vec<&ChatMessage> = messages.iter()
            .filter(|m| m.role != "system")
            .collect();
        
        if non_system.len() <= keep_recent {
            return Ok(messages);
        }
        
        let (old_msgs, recent_msgs) = non_system.split_at(non_system.len() - keep_recent);
        
        // Build summary of old messages for Haiku
        let mut summary_input = String::new();
        for m in old_msgs {
            let role_label = match m.role.as_str() {
                "user" => "User",
                "assistant" | "assistant_tool_use" => "Assistant",
                "tool_result" => "Tool Result",
                _ => &m.role,
            };
            // Truncate each message for the summary request
            let content = if m.content.len() > 500 {
                format!("{}...", &m.content[..500])
            } else {
                m.content.clone()
            };
            summary_input.push_str(&format!("[{}]: {}\n", role_label, content));
        }
        
        // Ask Haiku to summarize
        let compaction_prompt = format!(
            "Summarize this conversation concisely. The original task was: \"{}\"\n\n\
            Focus on: what was accomplished, what tools were used, key results, and what's still pending.\n\n\
            Conversation:\n{}",
            original_task,
            summary_input
        );
        
        let compact_request = ChatRequest {
            messages: vec![
                ChatMessage::user(&compaction_prompt),
            ],
            tools: None,
            model: COMPACTION_MODEL.to_string(),
            temperature: 0.3,
            max_tokens: Some(1000),
        };
        
        let summary = match self.provider.chat(&compact_request).await {
            Ok(resp) => resp.text.unwrap_or_else(|| "Failed to summarize.".to_string()),
            Err(e) => {
                tracing::warn!("Compaction failed: {}, falling back to truncation", e);
                // Fallback: just truncate old messages
                format!("[Previous {} messages truncated to save context]", old_msgs.len())
            }
        };
        
        // Rebuild messages: system + summary + recent
        let mut compacted = Vec::new();
        for sm in &system_msgs {
            compacted.push((*sm).clone());
        }
        compacted.push(ChatMessage::user(format!(
            "[Conversation compacted — {} earlier messages summarized]\n\n{}",
            old_msgs.len(), summary
        )));
        compacted.push(ChatMessage::assistant("Understood, continuing from the summary.".to_string()));
        for rm in recent_msgs {
            compacted.push((*rm).clone());
        }
        
        Ok(compacted)
    }
}
