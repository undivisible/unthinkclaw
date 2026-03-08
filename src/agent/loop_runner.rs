//! Agent loop — the core execution engine.
//! Processes incoming messages, calls LLM, executes tools, sends responses.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::{Channel, IncomingMessage, OutgoingMessage};
use crate::memory::MemoryBackend;
use crate::providers::{ChatMessage, ChatRequest, Provider};
use crate::skills;
use crate::tools::Tool;

/// Circuit breaker: stop after this many rounds to prevent infinite loops.
/// Unlike a hard limit, the LLM can run as many tools as needed.
/// We only stop if we detect a stuck loop (same tool+args repeating).
const CIRCUIT_BREAKER_ROUNDS: usize = 50;
/// Warn the LLM after this many identical tool calls
const LOOP_WARN_THRESHOLD: usize = 5;
/// Hard stop after this many identical consecutive tool calls
const LOOP_BREAK_THRESHOLD: usize = 10;

pub struct AgentRunner {
    provider: Arc<dyn Provider>,
    tools: Vec<Arc<dyn Tool>>,
    memory: Arc<dyn MemoryBackend>,
    system_prompt: String,
    model: String,
    workspace: PathBuf,
    skills: Vec<skills::Skill>,
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
        }
    }

    /// Set the workspace path (for skill matching).
    pub fn with_workspace(mut self, workspace: PathBuf) -> Self {
        self.workspace = workspace;
        self
    }

    /// Set discovered skills.
    pub fn with_skills(mut self, skills: Vec<skills::Skill>) -> Self {
        self.skills = skills;
        self
    }

    /// Run the agent loop on a channel.
    pub async fn run(&self, channel: &mut dyn Channel) -> anyhow::Result<()> {
        let mut rx = channel.start().await?;

        tracing::info!("Agent started on channel: {}", channel.name());

        while let Some(msg) = rx.recv().await {
            match self.handle_message(&msg).await {
                Ok(response) => {
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
    /// Messages from both the channel and the extra source are processed.
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

            match self.handle_message(&msg).await {
                Ok(response) => {
                    // Don't send heartbeat responses back to channel if it's a heartbeat message
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

    /// Handle a single message — LLM call with tool loop.
    async fn handle_message(&self, msg: &IncomingMessage) -> anyhow::Result<String> {
        // Build conversation with system prompt + memory context
        let mut messages = vec![ChatMessage::system(&self.system_prompt)];

        // Skill injection: check if user message matches a skill
        if let Some(skill) = skills::match_skill(&self.skills, &msg.text) {
            if let Some(content) = skills::load_skill_content(skill) {
                messages.push(ChatMessage::system(format!(
                    "# Active Skill: {}\n{}\n\nFollow the instructions above for this skill.",
                    skill.name, content
                )));
                tracing::info!("Skill matched: {}", skill.name);
            }
        }

        // Add memory context if available
        if let Ok(memories) = self.memory.search("chat", &msg.text, 5).await {
            if !memories.is_empty() {
                let context: Vec<String> = memories.iter()
                    .map(|m| format!("- {}: {}", m.key, m.value))
                    .collect();
                messages.push(ChatMessage::system(format!(
                    "Relevant past context:\n{}",
                    context.join("\n")
                )));
            }
        }

        messages.push(ChatMessage::user(&msg.text));

        // Tool specs for function calling
        let tool_specs: Vec<crate::tools::ToolSpec> = self.tools.iter()
            .map(|t| t.spec())
            .collect();

        // Agent loop: LLM → tool calls → LLM → ... until stop_reason=end_turn
        // No hard round limit. Loop detection catches stuck patterns.
        let mut tool_call_history: Vec<String> = Vec::new(); // hash of tool+args
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
                // No more tool calls — return the text response
                tracing::info!("Agent done after {} round(s)", round + 1);
                let text = response.text.unwrap_or_default();

                // Store the interaction in memory
                let _ = self.memory.store(
                    "chat",
                    &format!("msg_{}", msg.id),
                    &format!("User: {} | Assistant: {}", msg.text, &text[..text.len().min(200)]),
                    None,
                ).await;

                return Ok(text);
            }

            // Add assistant message with tool_use content blocks
            // Anthropic requires the full assistant response (text + tool_use blocks)
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
                // Store as assistant_tool_use with serialized content blocks
                messages.push(ChatMessage {
                    role: "assistant_tool_use".to_string(),
                    content: String::new(),
                    tool_use_id: Some(serde_json::to_string(&content_blocks).unwrap_or_default()),
                });
            }

            // Loop detection: hash tool calls and check for repeating patterns
            for tc in &response.tool_calls {
                let hash = format!("{}:{}", tc.name, tc.arguments);
                tool_call_history.push(hash);
            }
            // Check for stuck loops
            if tool_call_history.len() >= LOOP_BREAK_THRESHOLD {
                let last = &tool_call_history[tool_call_history.len() - 1];
                let consecutive = tool_call_history.iter().rev().take_while(|h| *h == last).count();
                if consecutive >= LOOP_BREAK_THRESHOLD {
                    tracing::warn!("Loop detected: {} identical calls to {}, breaking", consecutive, response.tool_calls[0].name);
                    return Ok(format!("I got stuck in a loop calling {} {} times. Let me try a different approach — can you rephrase your request?", response.tool_calls[0].name, consecutive));
                }
                if consecutive >= LOOP_WARN_THRESHOLD {
                    // Inject a warning into the conversation
                    messages.push(ChatMessage::user(format!(
                        "WARNING: You have called {} {} times with identical arguments. If this is not making progress, stop retrying and give me your best answer with what you have.",
                        response.tool_calls[0].name, consecutive
                    )));
                    tracing::warn!("Loop warning: {} identical calls to {}", consecutive, response.tool_calls[0].name);
                }
            }

            // Execute each tool call
            tracing::info!("Tool calls: {}", response.tool_calls.iter().map(|tc| tc.name.as_str()).collect::<Vec<_>>().join(", "));
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
        Ok("Hit the circuit breaker — too many tool rounds without finishing. This usually means I'm stuck. Try rephrasing?".to_string())
    }
}
