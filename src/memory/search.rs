//! Memory search — full-text search over MEMORY.md + memory/*.md files
//! Provides memory_search and memory_get as agent tools.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub path: String,
    pub line_number: usize,
    pub snippet: String,
    pub score: f32,
}

/// Search MEMORY.md + memory/*.md for a query string
pub fn memory_search(workspace: &Path, query: &str, max_results: usize) -> Vec<SearchResult> {
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let mut results = Vec::new();

    // Files to search
    let mut files: Vec<PathBuf> = Vec::new();

    // MEMORY.md
    let memory_md = workspace.join("MEMORY.md");
    if memory_md.exists() {
        files.push(memory_md);
    }

    // memory/*.md
    let memory_dir = workspace.join("memory");
    if memory_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&memory_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "md") {
                    files.push(path);
                }
            }
        }
    }

    for file in &files {
        if let Ok(content) = std::fs::read_to_string(file) {
            let rel_path = file
                .strip_prefix(workspace)
                .unwrap_or(file)
                .to_string_lossy()
                .to_string();

            for (i, line) in content.lines().enumerate() {
                let line_lower = line.to_lowercase();
                let mut score = 0.0f32;

                // Exact substring match
                if line_lower.contains(&query_lower) {
                    score += 10.0;
                }

                // Word-level matches
                for word in &query_words {
                    if line_lower.contains(word) {
                        score += 2.0;
                    }
                }

                if score > 0.0 {
                    // Get context (surrounding lines)
                    let lines: Vec<&str> = content.lines().collect();
                    let start = i.saturating_sub(1);
                    let end = (i + 2).min(lines.len());
                    let snippet = lines[start..end].join("\n");

                    results.push(SearchResult {
                        path: rel_path.clone(),
                        line_number: i + 1,
                        snippet,
                        score,
                    });
                }
            }
        }
    }

    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(max_results);
    results
}

/// Get specific lines from a memory file
pub fn memory_get(workspace: &Path, file_path: &str, from_line: usize, num_lines: usize) -> Option<String> {
    let full_path = workspace.join(file_path);

    // Security: ensure path stays within workspace
    let canonical = full_path.canonicalize().ok()?;
    let workspace_canonical = workspace.canonicalize().ok()?;
    if !canonical.starts_with(&workspace_canonical) {
        return None;
    }

    let content = std::fs::read_to_string(&full_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    let start = from_line.saturating_sub(1); // 1-indexed to 0-indexed
    let end = (start + num_lines).min(lines.len());

    if start >= lines.len() {
        return None;
    }

    Some(lines[start..end].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_memory_search_nonexistent() {
        let results = memory_search(&PathBuf::from("/nonexistent"), "test", 5);
        assert!(results.is_empty());
    }
}
