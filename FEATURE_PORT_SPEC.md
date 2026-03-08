# Feature Port Specification: OpenClaw → unthinkclaw

Port these 5 critical systems from OpenClaw into unthinkclaw (Rust).
All code goes in src/. Maintain the existing trait-based architecture.

## 1. System Prompt Builder (HIGHEST PRIORITY)
File: `src/prompt.rs`

Read and inject workspace context files into the system prompt:
- SOUL.md → personality/tone
- USER.md → user context  
- AGENTS.md → workspace rules
- MEMORY.md → long-term memory (only in main session)
- TOOLS.md → tool-specific notes
- IDENTITY.md → name/creature/vibe

Pattern from OpenClaw: concatenate all files into system prompt with headers.
The AgentRunner should call `build_system_prompt(workspace_path)` and prepend to messages.

## 2. Heartbeat System
File: `src/heartbeat.rs`

- Periodic timer (configurable interval, default 30min)
- Reads HEARTBEAT.md from workspace
- If HEARTBEAT.md has tasks → sends as user message to agent loop
- If empty/comments only → skip (no API call)
- Track last check timestamps in `memory/heartbeat-state.json`
- Respect quiet hours (23:00-08:00 unless urgent)
- Agent responds HEARTBEAT_OK to skip, or with text to send to user

## 3. Memory Search
File: `src/memory/search.rs`

- Scan MEMORY.md + memory/*.md files
- Full-text search (substring match + word boundary scoring)
- Return top N snippets with file path + line numbers
- Used as a tool: `memory_search(query) -> Vec<SearchResult>`
- Also `memory_get(path, from_line, num_lines) -> String`
- Register as tools in the agent loop

## 4. Skills System
File: `src/skills.rs`

- Scan ~/.npm-global/lib/node_modules/openclaw/skills/ AND ~/.openclaw/workspace/skills/
- Parse SKILL.md frontmatter: name, description, location
- Match user request against skill descriptions
- When matched: read SKILL.md content, inject into system prompt for that turn
- Skills are NOT executed — they're instruction sets the LLM follows

## 5. Cron Scheduler
File: `src/cron_scheduler.rs` (we already have src/scheduler.rs, extend it)

- Store jobs in SQLite (table: cron_jobs)
- Fields: id, name, schedule (cron expression), task (prompt text), channel, model, enabled, last_run, next_run
- Ticker checks every 60s for due jobs
- Due jobs → spawn a new agent session with the task prompt
- Support: add, list, remove, enable/disable via CLI subcommands

## Integration Points

### main.rs changes:
- Load system prompt from workspace files (call prompt::build_system_prompt)  
- Start heartbeat timer as background task
- Register memory_search + memory_get as tools
- Start cron scheduler as background task
- Add CLI subcommands: `cron add/list/remove`

### agent/loop_runner.rs changes:
- Accept dynamic system prompt (from prompt builder)
- Support injecting skill content per-turn

### Cargo.toml:
- Add `glob = "0.3"` for file scanning
- Add `regex = "1"` for search

## DO NOT:
- Change existing trait interfaces
- Break the existing CLI/Telegram/Discord channels
- Add heavy dependencies (keep binary <5MB)
- Over-engineer — simple working implementations first

## Build & Test:
```bash
cargo build --release 2>&1 | tail -20
# Must compile with 0 errors
# Binary must be < 5MB
```
