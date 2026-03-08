//! System prompt builder — reads SOUL.md, USER.md, AGENTS.md, MEMORY.md, TOOLS.md, IDENTITY.md
//! and assembles them into a system prompt for the LLM.

use std::path::Path;

/// Build the system prompt from workspace context files
pub fn build_system_prompt(workspace: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Identity (who am I)
    if let Some(content) = read_file(workspace, "IDENTITY.md") {
        parts.push(format!("## Identity\n{}", content));
    }

    // Soul (personality, tone, vibe)
    if let Some(content) = read_file(workspace, "SOUL.md") {
        parts.push(format!("## Personality & Tone\n{}", content));
    }

    // User context (who we're helping)
    if let Some(content) = read_file(workspace, "USER.md") {
        parts.push(format!("## About the User\n{}", content));
    }

    // Workspace rules
    if let Some(content) = read_file(workspace, "AGENTS.md") {
        parts.push(format!("## Workspace Rules\n{}", content));
    }

    // Tool-specific notes
    if let Some(content) = read_file(workspace, "TOOLS.md") {
        parts.push(format!("## Tool Notes\n{}", content));
    }

    // Long-term memory
    if let Some(content) = read_file(workspace, "MEMORY.md") {
        // Truncate to avoid blowing context
        let truncated = if content.len() > 8000 {
            format!("{}...\n(truncated)", &content[..8000])
        } else {
            content
        };
        parts.push(format!("## Long-Term Memory\n{}", truncated));
    }

    if parts.is_empty() {
        return "You are a helpful AI assistant.".to_string();
    }

    parts.join("\n\n---\n\n")
}

/// Read a file from workspace, return None if missing
fn read_file(workspace: &Path, filename: &str) -> Option<String> {
    let path = workspace.join(filename);
    std::fs::read_to_string(&path).ok().and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_system_prompt_empty_workspace() {
        let prompt = build_system_prompt(&PathBuf::from("/nonexistent"));
        assert_eq!(prompt, "You are a helpful AI assistant.");
    }
}
