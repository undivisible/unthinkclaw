//! Edit tool — surgical text replacement in files (like OpenClaw's Edit).
//! Finds exact text and replaces it, preserving the rest of the file.

use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

use super::traits::*;

pub struct EditTool {
    workspace: PathBuf,
}

impl EditTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[derive(Deserialize)]
struct EditArgs {
    /// File path (relative to workspace or absolute)
    #[serde(alias = "file_path")]
    path: String,
    /// Exact text to find (must match exactly including whitespace)
    #[serde(alias = "oldText")]
    old_string: String,
    /// New text to replace with
    #[serde(alias = "newText")]
    new_string: String,
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str { "Edit" }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "Edit".to_string(),
            description: "Edit a file by replacing exact text. The old_string must match exactly (including whitespace). Use this for precise, surgical edits.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to edit (relative to workspace or absolute)"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Exact text to find and replace (must match exactly)"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "New text to replace the old text with"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> anyhow::Result<ToolResult> {
        let args: EditArgs = serde_json::from_str(arguments)?;

        // Resolve path
        let full_path = if args.path.starts_with('/') {
            PathBuf::from(&args.path)
        } else {
            self.workspace.join(&args.path)
        };

        // Security: check path is within workspace for relative paths
        if !args.path.starts_with('/') {
            if let (Ok(canonical_ws), Ok(canonical_file)) = (self.workspace.canonicalize(), full_path.canonicalize()) {
                if !canonical_file.starts_with(&canonical_ws) {
                    return Ok(ToolResult::error("Path traversal not allowed"));
                }
            }
        }

        // Read file
        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Cannot read file '{}': {}", args.path, e))),
        };

        // Find and replace
        if !content.contains(&args.old_string) {
            return Ok(ToolResult::error(format!(
                "Could not find the exact text in {}. The old_string must match exactly including all whitespace and newlines.",
                args.path
            )));
        }

        let new_content = content.replacen(&args.old_string, &args.new_string, 1);

        // Write back
        match tokio::fs::write(&full_path, &new_content).await {
            Ok(_) => Ok(ToolResult::success(format!("Successfully replaced text in {}", args.path))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write: {}", e))),
        }
    }
}
