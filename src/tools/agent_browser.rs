//! Agent-browser tool — LLM-driven web automation via agent-browser CLI
//! https://github.com/vercel/agent-browser

use super::traits::{Tool, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Response from agent-browser --json commands
#[derive(Debug, Deserialize)]
struct AgentBrowserResponse {
    success: bool,
    data: Option<Value>,
    error: Option<String>,
}

/// Supported browser actions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAction {
    /// Navigate to a URL
    Open { url: String },
    /// Get accessibility snapshot with refs
    Snapshot {
        #[serde(default)]
        interactive_only: bool,
    },
    /// Click an element by selector
    Click { selector: String },
    /// Fill a form field
    Fill { selector: String, value: String },
    /// Type text
    Type { text: String },
    /// Get page title
    GetTitle,
    /// Get current URL
    GetUrl,
    /// Take screenshot
    Screenshot {
        #[serde(default)]
        path: Option<String>,
    },
    /// Wait for element or time
    Wait {
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        ms: Option<u64>,
    },
    /// Press a key
    Press { key: String },
    /// Scroll page
    Scroll { direction: String },
    /// Close browser
    Close,
}

pub struct AgentBrowserTool {
    command: String,
    session_name: Option<String>,
    timeout_ms: u64,
}

impl AgentBrowserTool {
    pub fn new() -> Self {
        Self {
            command: "agent-browser".to_string(),
            session_name: None,
            timeout_ms: 30_000,
        }
    }

    pub fn with_session(mut self, session: impl Into<String>) -> Self {
        self.session_name = Some(session.into());
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Check if agent-browser CLI is available
    pub async fn is_available() -> bool {
        Command::new("agent-browser")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Execute an agent-browser command
    async fn run_command(&self, args: &[&str]) -> anyhow::Result<AgentBrowserResponse> {
        let mut cmd = Command::new(&self.command);

        // Add session if configured
        if let Some(ref session) = self.session_name {
            cmd.arg("--session").arg(session);
        }

        // Add --json for machine-readable output
        cmd.args(args).arg("--json");

        tracing::debug!("Running: {} {} --json", self.command, args.join(" "));

        let output = tokio::time::timeout(
            Duration::from_millis(self.timeout_ms),
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output(),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "agent-browser command timed out after {} ms",
                self.timeout_ms
            )
        })??;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stderr.is_empty() {
            tracing::debug!("agent-browser stderr: {}", stderr);
        }

        // Parse JSON response
        if let Ok(resp) = serde_json::from_str::<AgentBrowserResponse>(&stdout) {
            return Ok(resp);
        }

        // Fallback for non-JSON output
        if output.status.success() {
            Ok(AgentBrowserResponse {
                success: true,
                data: Some(json!({ "output": stdout.trim() })),
                error: None,
            })
        } else {
            Ok(AgentBrowserResponse {
                success: false,
                data: None,
                error: Some(stderr.to_string()),
            })
        }
    }

    async fn execute_action(&self, action: &BrowserAction) -> anyhow::Result<String> {
        match action {
            BrowserAction::Open { url } => {
                let resp = self.run_command(&["open", url]).await?;
                if resp.success {
                    Ok(format!("Opened: {}", url))
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Snapshot { interactive_only } => {
                let mut args = vec!["snapshot"];
                if *interactive_only {
                    args.push("--interactive-only");
                }
                let resp = self.run_command(&args).await?;
                if resp.success {
                    Ok(serde_json::to_string_pretty(&resp.data.unwrap_or_default())?)
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Click { selector } => {
                let resp = self.run_command(&["click", selector]).await?;
                if resp.success {
                    Ok(format!("Clicked: {}", selector))
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Fill { selector, value } => {
                let resp = self.run_command(&["fill", selector, value]).await?;
                if resp.success {
                    Ok(format!("Filled: {} = {}", selector, value))
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Type { text } => {
                let resp = self.run_command(&["type", text]).await?;
                if resp.success {
                    Ok(format!("Typed: {}", text))
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::GetTitle => {
                let resp = self.run_command(&["title"]).await?;
                if resp.success {
                    Ok(resp.data.and_then(|d| d.as_str().map(String::from)).unwrap_or_default())
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::GetUrl => {
                let resp = self.run_command(&["url"]).await?;
                if resp.success {
                    Ok(resp.data.and_then(|d| d.as_str().map(String::from)).unwrap_or_default())
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Screenshot { path } => {
                let mut args = vec!["screenshot"];
                if let Some(p) = path {
                    args.extend(&["--path", p]);
                }
                let resp = self.run_command(&args).await?;
                if resp.success {
                    Ok(format!("Screenshot saved: {}", path.as_deref().unwrap_or("screenshot.png")))
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Wait { selector, ms } => {
                let mut args = vec!["wait"];
                if let Some(sel) = selector {
                    args.extend(&["--selector", sel]);
                }
                if let Some(delay) = ms {
                    args.extend(&["--ms", &delay.to_string()]);
                }
                let resp = self.run_command(&args).await?;
                if resp.success {
                    Ok("Wait complete".to_string())
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Press { key } => {
                let resp = self.run_command(&["press", key]).await?;
                if resp.success {
                    Ok(format!("Pressed: {}", key))
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Scroll { direction } => {
                let resp = self.run_command(&["scroll", direction]).await?;
                if resp.success {
                    Ok(format!("Scrolled: {}", direction))
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
            BrowserAction::Close => {
                let resp = self.run_command(&["close"]).await?;
                if resp.success {
                    Ok("Browser closed".to_string())
                } else {
                    Err(anyhow::anyhow!("{}", resp.error.unwrap_or_default()))
                }
            }
        }
    }
}

#[async_trait]
impl Tool for AgentBrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Control a web browser via agent-browser CLI. Supports navigation, element interaction, screenshots, and accessibility snapshots."
    }

    fn spec(&self) -> crate::tools::ToolSpec {
        crate::tools::ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "object",
                        "oneOf": [
                            {
                                "type": "object",
                                "required": ["open", "url"],
                                "properties": {
                                    "open": { "type": "object" },
                                    "url": { "type": "string", "description": "URL to navigate to" }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["snapshot"],
                                "properties": {
                                    "snapshot": {
                                        "type": "object",
                                        "properties": {
                                            "interactive_only": { "type": "boolean", "default": false }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["click", "selector"],
                                "properties": {
                                    "click": { "type": "object" },
                                    "selector": { "type": "string", "description": "CSS selector or element ref" }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["fill", "selector", "value"],
                                "properties": {
                                    "fill": { "type": "object" },
                                    "selector": { "type": "string" },
                                    "value": { "type": "string" }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["type", "text"],
                                "properties": {
                                    "type": { "type": "object" },
                                    "text": { "type": "string" }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["get_title"],
                                "properties": {
                                    "get_title": { "type": "object" }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["get_url"],
                                "properties": {
                                    "get_url": { "type": "object" }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["screenshot"],
                                "properties": {
                                    "screenshot": {
                                        "type": "object",
                                        "properties": {
                                            "path": { "type": "string" }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["wait"],
                                "properties": {
                                    "wait": {
                                        "type": "object",
                                        "properties": {
                                            "selector": { "type": "string" },
                                            "ms": { "type": "integer" }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["press", "key"],
                                "properties": {
                                    "press": { "type": "object" },
                                    "key": { "type": "string", "description": "Key name (e.g. Enter, Escape)" }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["scroll", "direction"],
                                "properties": {
                                    "scroll": { "type": "object" },
                                    "direction": { "type": "string", "enum": ["up", "down", "left", "right"] }
                                }
                            },
                            {
                                "type": "object",
                                "required": ["close"],
                                "properties": {
                                    "close": { "type": "object" }
                                }
                            }
                        ]
                    }
                }
            }),
        }
    }

    async fn execute(&self, args: &str) -> anyhow::Result<ToolResult> {
        let parsed: Value = serde_json::from_str(args)?;
        let action: BrowserAction = serde_json::from_value(parsed["action"].clone())?;

        match self.execute_action(&action).await {
            Ok(output) => Ok(ToolResult::success(output)),
            Err(e) => Ok(ToolResult::error(format!("Browser error: {}", e))),
        }
    }
}
