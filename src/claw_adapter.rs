//! Claw adapter — migrate from OpenClaw to aclaw
//! Maps SOUL.md, AGENTS.md config to aclaw Config
//! Enables existing Claw workflows to run on aclaw

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Map SOUL.md personality
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Soul {
    pub name: String,
    pub vibe: String,
}

/// Map USER.md context
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub name: String,
    pub location: String,
    pub languages: Vec<String>,
    pub interests: Vec<String>,
}

/// Map AGENTS.md workspace
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentsConfig {
    pub soul: Soul,
    pub user: User,
    pub workspace: String,
    pub memory_dir: String,
    pub projects: Vec<ProjectRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectRef {
    pub name: String,
    pub path: String,
}

impl AgentsConfig {
    /// Load from SOUL.md + USER.md + AGENTS.md
    pub fn load(workspace: &str) -> anyhow::Result<Self> {
        let workspace_path = Path::new(workspace);

        // Read SOUL.md
        let soul_content = std::fs::read_to_string(workspace_path.join("SOUL.md"))
            .unwrap_or_default();
        let soul = Soul {
            name: extract_field(&soul_content, "Name"),
            vibe: extract_field(&soul_content, "Vibe"),
        };

        // Read USER.md
        let user_content = std::fs::read_to_string(workspace_path.join("USER.md"))
            .unwrap_or_default();
        let user = User {
            name: extract_field(&user_content, "Name"),
            location: extract_field(&user_content, "Location"),
            languages: vec![],
            interests: vec![],
        };

        Ok(Self {
            soul,
            user,
            workspace: workspace.to_string(),
            memory_dir: workspace_path.join("memory").to_string_lossy().to_string(),
            projects: vec![],
        })
    }

    /// Convert to aclaw system prompt
    pub fn to_system_prompt(&self) -> String {
        format!(
            "You are {}. {}\n\nWorking for: {}\nLocation: {}\n\nStyle: {}",
            self.soul.name, self.soul.vibe, self.user.name, self.user.location, self.soul.vibe
        )
    }

    /// Get claw configuration
    pub fn claw_config(&self) -> String {
        format!(
            r#"{{
  "claw": {{
    "name": "{}",
    "vibe": "{}",
    "user": {{
      "name": "{}",
      "location": "{}"
    }},
    "workspace": "{}",
    "memory_dir": "{}"
  }}
}}"#,
            self.soul.name,
            self.soul.vibe,
            self.user.name,
            self.user.location,
            self.workspace,
            self.memory_dir
        )
    }
}

fn extract_field(content: &str, field: &str) -> String {
    let bold_key = format!("**{}:**", field);
    let plain_key = format!("{}:", field);
    for line in content.lines() {
        let after = [bold_key.as_str(), plain_key.as_str()]
            .iter()
            .find_map(|key| line.find(key).map(|i| &line[i + key.len()..]));
        if let Some(value) = after {
            return value.trim().to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_field() {
        let content = r#"**Name:** Claw
**Vibe:** Smartass best friend who knows everything."#;

        assert_eq!(extract_field(content, "Name"), "Claw");
        assert_eq!(
            extract_field(content, "Vibe"),
            "Smartass best friend who knows everything."
        );
    }

    #[test]
    fn test_system_prompt() {
        let config = AgentsConfig {
            soul: Soul {
                name: "Claw".to_string(),
                vibe: "Smartass helper".to_string(),
            },
            user: User {
                name: "Max".to_string(),
                location: "Melbourne".to_string(),
                languages: vec![],
                interests: vec![],
            },
            workspace: "/tmp".to_string(),
            memory_dir: "/tmp/memory".to_string(),
            projects: vec![],
        };

        let prompt = config.to_system_prompt();
        assert!(prompt.contains("Claw"));
        assert!(prompt.contains("Max"));
        assert!(prompt.contains("Melbourne"));
    }
}
