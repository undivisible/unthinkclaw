//! File operations tool — read, write, list files.

use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

use super::traits::*;

pub struct FileReadTool {
    workspace: PathBuf,
}

impl FileReadTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str { "file_read" }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "file_read".to_string(),
            description: "Read a file's contents".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to workspace" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> anyhow::Result<ToolResult> {
        let args: ReadArgs = serde_json::from_str(arguments)?;
        let full_path = self.workspace.join(&args.path);

        // Canonicalize both paths to resolve symlinks and prevent traversal.
        // The file must exist for canonicalize to succeed, which also acts as an
        // existence check before we attempt the read.
        let canonical_workspace = match self.workspace.canonicalize() {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(format!("Workspace inaccessible: {}", e))),
        };
        let canonical_path = match full_path.canonicalize() {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(format!("Cannot access path '{}': {}", args.path, e))),
        };
        if !canonical_path.starts_with(&canonical_workspace) {
            return Ok(ToolResult::error("Path traversal not allowed"));
        }

        match tokio::fs::read_to_string(&full_path).await {
            Ok(content) => {
                let truncated = if content.len() > 50_000 {
                    format!("{}...\n[truncated]", &content[..50_000])
                } else {
                    content
                };
                Ok(ToolResult::success(truncated))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to read: {}", e))),
        }
    }
}

pub struct FileWriteTool {
    workspace: PathBuf,
}

impl FileWriteTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str { "file_write" }

    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "file_write".to_string(),
            description: "Write content to a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to workspace" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> anyhow::Result<ToolResult> {
        let args: WriteArgs = serde_json::from_str(arguments)?;
        let full_path = self.workspace.join(&args.path);

        // Canonicalize the workspace; for the target path canonicalize the
        // parent (the file may not exist yet) and reconstruct with the filename.
        let canonical_workspace = match self.workspace.canonicalize() {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(format!("Workspace inaccessible: {}", e))),
        };
        let parent = full_path.parent().unwrap_or(&full_path);
        // Ensure parent dirs exist before canonicalizing them.
        tokio::fs::create_dir_all(parent).await?;
        let canonical_parent = match parent.canonicalize() {
            Ok(p) => p,
            Err(e) => return Ok(ToolResult::error(format!("Cannot access parent directory for '{}': {}", args.path, e))),
        };
        let filename = match full_path.file_name() {
            Some(n) => n,
            None => return Ok(ToolResult::error("Path has no filename")),
        };
        let canonical_path = canonical_parent.join(filename);
        if !canonical_path.starts_with(&canonical_workspace) {
            return Ok(ToolResult::error("Path traversal not allowed"));
        }

        match tokio::fs::write(&canonical_path, &args.content).await {
            Ok(_) => Ok(ToolResult::success(format!("Wrote {} bytes to {}", args.content.len(), args.path))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write: {}", e))),
        }
    }
}
