//! Shell/exec tool — execute commands (matches OpenClaw's exec tool).

use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

use super::traits::*;

pub struct ShellTool {
    workspace: PathBuf,
    timeout_secs: u64,
}

impl ShellTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace, timeout_secs: 120 }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

#[derive(Deserialize)]
struct ShellArgs {
    command: String,
    /// Working directory (defaults to workspace)
    #[serde(alias = "workdir")]
    cwd: Option<String>,
    /// Timeout in seconds
    timeout: Option<u64>,
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str { "exec" }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "exec".to_string(),
            description: "Execute shell commands. Returns stdout/stderr and exit code.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory (defaults to workspace)"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds (default 120)"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> anyhow::Result<ToolResult> {
        let args: ShellArgs = serde_json::from_str(arguments)?;
        let timeout = args.timeout.unwrap_or(self.timeout_secs);

        let cwd = if let Some(dir) = &args.cwd {
            if dir.starts_with('/') {
                PathBuf::from(dir)
            } else {
                self.workspace.join(dir)
            }
        } else {
            self.workspace.clone()
        };

        let child = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(&args.command)
            .current_dir(&cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        let output = match tokio::time::timeout(
            Duration::from_secs(timeout),
            child.wait_with_output(),
        ).await {
            Ok(result) => result?,
            Err(_) => {
                return Ok(ToolResult::error(format!(
                    "Command timed out after {}s", timeout
                )));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let result = if stdout.is_empty() && !stderr.is_empty() {
            stderr.to_string()
        } else if !stderr.is_empty() {
            format!("{}\n{}", stdout, stderr)
        } else {
            stdout.to_string()
        };

        // Truncate if too long
        let truncated = if result.len() > 20_000 {
            format!("{}...\n[truncated {} chars]", &result[..20_000], result.len() - 20_000)
        } else {
            result
        };

        Ok(if output.status.success() {
            ToolResult::success(truncated)
        } else {
            ToolResult::error(format!(
                "Exit code {}: {}",
                output.status.code().unwrap_or(-1),
                truncated
            ))
        })
    }
}
