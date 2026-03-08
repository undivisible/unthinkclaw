//! Agent loop — the core execution engine.
//! Processes incoming messages, calls LLM, executes tools, sends responses.
//! Supports progress callbacks for real-time feedback.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::{Channel, IncomingMessage, OutgoingMessage};
use crate::memory::MemoryBackend;
use crate::providers::{ChatMessage, ChatRequest, Provider};
use crate::skills;
use crate::tools::Tool;

/// Circuit breaker: stop after this many rounds to prevent infinite loops.
const CIRCUIT_BREAKER_ROUNDS: usize = 50;
/// Warn the LLM after this many identical tool calls
const LOOP_WARN_THRESHOLD: usize = 5;
/// Hard stop after this many identical consecutive tool calls
const LOOP_BREAK_THRESHOLD: usize = 10;
/// Max conversation history to keep (prevents context overflow)
const MAX_HISTORY_MESSAGES: usize = 40;

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
    tools: Vec<Arc<dyn Tool>>,
    memory: Arc<dyn MemoryBackend>,
    system_prompt: String,
    model: String,
    workspace: PathBuf,
    skills: Vec<skills::Skill>,
    /// Conversation history (persisted across messages)
    conversation_history: std::sync::Mutex<Vec<ChatMessage>>,
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
            tools,
            memory,
            system_prompt: system_prompt.into(),
            model: model.into(),
            workspace: PathBuf::from("."),
            skills: Vec::new(),
            conversation_history: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn with_workspace(mut self, workspace: PathBuf) -> Self {
        self.workspace = workspace;
        self
    }

    pub fn with_skills(mut self, skills: Vec<skills::Skill>) -> Self {
        self.skills = skills;
        self
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
        let (tx, _rx) = mpsc::channel(32);
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
        let mut messages = vec![ChatMessage::system(&self.system_prompt)];

        // Skill injection
        if let Some(skill) = skills::match_skill(&self.skills, &msg.text) {
            if let Some(content) = skills::load_skill_content(skill) {
                messages.push(ChatMessage::system(format!(
                    "# Active Skill: {}\n{}\n\nFollow the instructions above for this skill.",
                    skill.name, content
                )));
                tracing::info!("Skill matched: {}", skill.name);
            }
        }

        // Add conversation history (keeps context across messages)
        {
            let history = self.conversation_history.lock().unwrap();
            for msg in history.iter() {
                messages.push(msg.clone());
            }
        }

        // Add new user message
        messages.push(ChatMessage::user(&msg.text));

        // Tool specs
        let tool_specs: Vec<crate::tools::ToolSpec> = self.tools.iter()
            .map(|t| t.spec())
            .collect();

        // Agent loop: unlimited rounds with loop detection
        let mut tool_call_history: Vec<String> = Vec::new();
        for round in 0..CIRCUIT_BREAKER_ROUNDS {
            tracing::info!("Agent round {} — {} messages", round + 1, messages.len());

            let request = ChatRequest {
                messages: messages.clone(),
                tools: if tool_specs.is_empty() { None } else { Some(tool_specs.clone()) },
                model: self.model.clone(),
                temperature: 0.7,
                max_tokens: None,
            };

            let response = self.provider.chat(&request).await?;

            if !response.has_tool_calls() {
                // Done — return text and persist to history
                tracing::info!("Agent done after {} round(s)", round + 1);
                let text = response.text.unwrap_or_default();

                // Persist conversation history
                self.add_to_history(
                    ChatMessage::user(&msg.text),
                    ChatMessage::assistant(&text),
                );

                // Store in SQLite memory
                let _ = self.memory.store(
                    "chat",
                    &format!("msg_{}", msg.id),
                    &format!("User: {} | Assistant: {}", msg.text, &text[..text.len().min(500)]),
                    None,
                ).await;

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
                let result = if let Some(tool) = self.tools.iter().find(|t| t.name() == tc.name) {
                    match tool.execute(&tc.arguments).await {
                        Ok(r) => r,
                        Err(e) => crate::tools::ToolResult::error(format!("Tool error: {}", e)),
                    }
                } else {
                    crate::tools::ToolResult::error(format!("Unknown tool: {}", tc.name))
                };

                messages.push(ChatMessage::tool_result(&tc.id, &result.output));
            }
        }

        tracing::error!("Circuit breaker: {} rounds without completion", CIRCUIT_BREAKER_ROUNDS);
        Ok("Hit the circuit breaker. Try rephrasing?".to_string())
    }

    /// Add user/assistant pair to conversation history, trimming to MAX_HISTORY
    fn add_to_history(&self, user_msg: ChatMessage, assistant_msg: ChatMessage) {
        let mut history = self.conversation_history.lock().unwrap();
        history.push(user_msg);
        history.push(assistant_msg);

        // Trim oldest messages if too long
        while history.len() > MAX_HISTORY_MESSAGES {
            history.remove(0);
        }
    }
}
