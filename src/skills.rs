//! Skills system — scan SKILL.md files, match against user requests,
//! inject matched skill instructions into the system prompt.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub location: PathBuf,
}

/// Scan known skill directories for SKILL.md files
pub fn discover_skills() -> Vec<Skill> {
    let mut skills = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();

    // OpenClaw bundled skills
    let openclaw_skills = home.join(".npm-global/lib/node_modules/openclaw/skills");
    scan_skill_dir(&openclaw_skills, &mut skills);

    // User workspace skills
    let workspace_skills = home.join(".openclaw/workspace/skills");
    scan_skill_dir(&workspace_skills, &mut skills);

    skills
}

fn scan_skill_dir(dir: &Path, skills: &mut Vec<Skill>) {
    if !dir.is_dir() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let skill_dir = entry.path();
            if !skill_dir.is_dir() {
                continue;
            }

            let skill_md = skill_dir.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                if let Some(skill) = parse_skill_frontmatter(&content, &skill_md) {
                    skills.push(skill);
                }
            }
        }
    }
}

/// Parse YAML-like frontmatter from SKILL.md
fn parse_skill_frontmatter(content: &str, path: &Path) -> Option<Skill> {
    // Look for --- delimited frontmatter
    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = &rest[..end];

    let mut name = None;
    let mut description = None;

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("name:") {
            name = Some(val.trim().trim_matches('\'').trim_matches('"').to_string());
        }
        if let Some(val) = trimmed.strip_prefix("description:") {
            description = Some(val.trim().trim_matches('\'').trim_matches('"').to_string());
        }
    }

    Some(Skill {
        name: name.unwrap_or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        }),
        description: description.unwrap_or_default(),
        location: path.to_path_buf(),
    })
}

/// Find the best matching skill for a user message
/// Stopwords — common words that should never contribute to skill matching
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had",
    "her", "was", "one", "our", "out", "has", "have", "been", "some", "them",
    "than", "its", "over", "such", "that", "this", "with", "will", "each",
    "from", "they", "were", "which", "their", "said", "what", "when", "who",
    "how", "use", "new", "now", "way", "may", "get", "got", "set", "let",
    "any", "also", "into", "just", "only", "very", "even", "most", "other",
    "need", "make", "like", "does", "your", "more", "want", "should",
];

fn is_stopword(word: &str) -> bool {
    STOPWORDS.contains(&word)
}

pub fn match_skill<'a>(skills: &'a [Skill], user_message: &str) -> Option<&'a Skill> {
    let msg_lower = user_message.to_lowercase();
    let msg_words: Vec<&str> = msg_lower.split_whitespace().collect();

    let mut best_score = 0.0f32;
    let mut best_skill = None;

    for skill in skills {
        let desc_lower = skill.description.to_lowercase();
        let name_lower = skill.name.to_lowercase();
        let mut score = 0.0f32;

        // Direct name mention (strong signal)
        if msg_lower.contains(&name_lower) && name_lower.len() >= 4 {
            score += 10.0;
        }

        // Word overlap: message words in description (skip stopwords)
        for word in &msg_words {
            if word.len() < 4 { continue; }
            if is_stopword(word) { continue; }
            if desc_lower.contains(word) {
                score += 1.0;
            }
        }

        // Word overlap: description words in message (skip stopwords)
        for word in desc_lower.split(|c: char| !c.is_alphanumeric()) {
            if word.len() < 4 { continue; }
            if is_stopword(word) { continue; }
            if msg_lower.contains(word) {
                score += 1.0;
            }
        }

        if score > best_score {
            best_score = score;
            best_skill = Some(skill);
        }
    }

    // Require strong match
    if best_score >= 5.0 {
        best_skill
    } else {
        None
    }
}

/// Load the full content of a skill's SKILL.md
pub fn load_skill_content(skill: &Skill) -> Option<String> {
    std::fs::read_to_string(&skill.location).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let content = "---\nname: test-skill\ndescription: A test skill\n---\n# Content";
        let skill = parse_skill_frontmatter(content, Path::new("/tmp/test/SKILL.md"));
        assert!(skill.is_some());
        let s = skill.unwrap();
        assert_eq!(s.name, "test-skill");
        assert_eq!(s.description, "A test skill");
    }

    #[test]
    fn test_match_skill() {
        let skills = vec![
            Skill {
                name: "weather".to_string(),
                description: "Get weather forecasts for any location".to_string(),
                location: PathBuf::from("/tmp/weather/SKILL.md"),
            },
            Skill {
                name: "github".to_string(),
                description: "GitHub operations, PRs, issues, code review".to_string(),
                location: PathBuf::from("/tmp/github/SKILL.md"),
            },
        ];

        let matched = match_skill(&skills, "what's the weather in Melbourne?");
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name, "weather");

        let matched = match_skill(&skills, "review the github issues");
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name, "github");
    }
}
