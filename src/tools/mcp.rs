//! MCP tool — Integrate Codex (or any MCP server) as a tool

use async_trait::async_trait;
use serde::Deserialize;

use super::traits::*;
use crate::mcp::CodexClient;

pub struct McpTool {
    client: Option<CodexClient>,
}

impl McpTool {
    pub fn new() -> Self {
        Self { client: None }
    }

    async fn ensure_client(&mut self) -> anyhow::Result<&CodexClient> {
        if self.client.is_none() {
            let client = CodexClient::spawn().await?;
            self.client = Some(client);
        }

        self.client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Failed to initialize Codex client"))
    }
}

#[derive(Deserialize)]
struct McpArgs {
    /// Action: run (coding session), list_tools
    action: String,
    /// Goal/task description (for run action)
    goal: Option<String>,
    /// Repository path (for run action)
    path: Option<String>,
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str { "mcp" }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "mcp".to_string(),
            description: "Interact with Codex via MCP (Model Context Protocol). Actions: run (start coding session), list_tools (show available Codex tools).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["run", "list_tools"],
                        "description": "MCP action"
                    },
                    "goal": {
                        "type": "string",
                        "description": "Coding goal/task (for run action)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Repository path (for run action, defaults to current directory)"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> anyhow::Result<ToolResult> {
        let args: McpArgs = serde_json::from_str(arguments)?;

        // This is a bit hacky since Tool trait doesn't support &mut self
        // In production, you'd use Arc<Mutex<CodexClient>> or lazy_static
        // For now, spawn a new client each time
        let client = CodexClient::spawn().await
            .map_err(|e| anyhow::anyhow!("Failed to spawn Codex: {}. Install with: cargo install --git https://github.com/atechnology-company/vibemania codex", e))?;

        match args.action.as_str() {
            "run" => {
                let goal = args.goal
                    .ok_or_else(|| anyhow::anyhow!("goal required for run action"))?;
                let path = args.path.unwrap_or_else(|| ".".to_string());

                let result = client.run_session(&goal, &path).await?;
                
                client.shutdown().await.ok(); // Best effort cleanup

                Ok(ToolResult::success(serde_json::to_string_pretty(&result)?))
            }

            "list_tools" => {
                let tools = client.list_tools().await?;
                
                client.shutdown().await.ok();

                let output = tools.iter()
                    .map(|t| format!("- {}: {}", t.name, t.description.as_deref().unwrap_or("No description")))
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(ToolResult::success(output))
            }

            other => Ok(ToolResult::error(format!("Unknown action: {}. Use: run, list_tools", other))),
        }
    }
}
