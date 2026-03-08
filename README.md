# unthinkclaw

Lightweight agent runtime. Unthink everything.

## Features

- Multi-provider (Anthropic, OpenAI, Ollama, OpenRouter, ...)
- Multi-channel (Telegram, Discord, CLI, ...)
- Agent swarms with parallel execution
- SQLite memory with FTS5, embeddings, conversation history
- Cron scheduling, cost tracking, plugin system
- WAL-mode SQLite, all DB ops on blocking thread pool

## Build

```bash
cargo build --release
```

## Run

```bash
./target/release/unthinkclaw --config openclaw.json
```

## Config

See `openclaw.json`. Set `ANTHROPIC_API_KEY` (or relevant provider key) in env.
