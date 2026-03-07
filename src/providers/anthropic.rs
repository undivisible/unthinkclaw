//! Anthropic (Claude) provider implementation.
//! Supports both API keys and OAuth tokens (Claude.dev)

use async_trait::async_trait;
use serde_json::Value;

use super::traits::*;
use crate::tools::ToolSpec;

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
        }
    }

    /// Create from OAuth token (Claude.dev) or fallback to environment/file
    pub fn from_env_or_oauth() -> anyhow::Result<Self> {
        // Try standard API key first
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            return Ok(Self::new(key));
        }

        // Try loading from Claude.dev OAuth credentials
        if let Ok((token, _, _)) = super::oauth::load_oauth_token_from_file() {
            return Ok(Self::new(token));
        }

        Err(anyhow::anyhow!(
            "No ANTHROPIC_API_KEY found. Set env var or install Claude for Desktop with OAuth token."
        ))
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    fn build_tools_payload(&self, tools: &[ToolSpec]) -> Vec<Value> {
        tools.iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        }).collect()
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str { "anthropic" }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tools: true,
            streaming: true,
            vision: true,
            max_context: 200_000,
        }
    }

    async fn chat(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        // Create client with 120s socket timeout (LLM calls can be slow)
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        // Split system message from conversation
        let system = request.messages.iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        let messages: Vec<Value> = request.messages.iter()
            .filter(|m| m.role != "system")
            .map(|m| serde_json::json!({ "role": &m.role, "content": &m.content }))
            .collect();

        let mut body = serde_json::json!({
            "model": &request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "temperature": request.temperature,
        });

        if let Some(sys) = system {
            body["system"] = Value::String(sys);
        }

        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                body["tools"] = Value::Array(self.build_tools_payload(tools));
            }
        }

        // Detect OAuth tokens (sk-ant-oat) vs API keys (sk-ant-api)
        // OAuth tokens use Authorization: Bearer header
        // API keys use x-api-key header
        let is_oauth = self.api_key.contains("sk-ant-oat");

        let mut req_builder = client
            .post(format!("{}/messages", self.base_url))
            .header("content-type", "application/json")
            .header("anthropic-version", "2023-06-01");

        if is_oauth {
            // OAuth token: use Bearer auth + required beta headers
            req_builder = req_builder
                .header("Authorization", format!("Bearer {}", &self.api_key))
                .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14");
        } else {
            // Standard API key
            req_builder = req_builder
                .header("x-api-key", &self.api_key);
        }

        let resp = req_builder
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error {}: {}", status, &text[..text.len().min(200)]);
        }

        let data: Value = resp.json().await?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(content) = data["content"].as_array() {
            for block in content {
                match block["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = block["text"].as_str() {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        tool_calls.push(ToolCall {
                            id: block["id"].as_str().unwrap_or("").to_string(),
                            name: block["name"].as_str().unwrap_or("").to_string(),
                            arguments: block["input"].to_string(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let usage = data["usage"].as_object().map(|u| Usage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            text: if text_parts.is_empty() { None } else { Some(text_parts.join("")) },
            tool_calls,
            usage,
        })
    }
}
