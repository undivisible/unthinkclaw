# unthinkclaw

Hosted and multi-user unthinkclaw platform branch.

## Branches

- `main` is the device-first bot branch for a single user running locally.
- `codex/full-platform` is the hosted gateway, web app, live session, and
  deployment branch.

## Features

- Multi-provider (Anthropic, OpenAI, Ollama, OpenRouter, ...)
- Multi-channel (Telegram, Discord, CLI, ...)
- Agent swarms with parallel execution
- SurrealDB/Rocks-backed hosted state plus local memory backends
- Cron scheduling, cost tracking, plugin system
- WAL-mode SQLite, all DB ops on blocking thread pool
- Web app, hosted gateway, and deployment manifests

## Build

```bash
cargo build --release
```

## Run

```bash
./target/release/unthinkclaw gateway --config unthinkclaw.json
```

## Config

Create `unthinkclaw.json` with `unthinkclaw init`, then set `ANTHROPIC_API_KEY`
(or the relevant provider key) in your environment.

For local-only single-device use, use the `main` branch instead of this one.
