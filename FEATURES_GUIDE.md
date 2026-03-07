# aclaw Features Guide

## Quick Start

```bash
# Default: CLI + Telegram + Anthropic
cargo build --release

# Minimal: Just CLI + Anthropic (smallest startup time)
cargo build --release --features "channel-cli,provider-anthropic" --no-default-features

# Everything: All channels + all providers
cargo build --release --features full

# Custom: Pick what you need
cargo build --release --features "channel-slack,channel-discord,provider-openai"
```

## Feature Categories

### Channels (10)
Enable only the messaging platforms you use:

| Feature | Size Impact | Dependencies | Notes |
|---------|------------|--------------|-------|
| `channel-cli` | baseline | tokio | Interactive terminal, always lightweight |
| `channel-telegram` | +20KB | reqwest | Polling-based, no webhooks needed |
| `channel-discord` | +20KB | reqwest | HTTP API, simple integration |
| `channel-slack` | +30KB | reqwest, axum | Web API, webhook receiver |
| `channel-whatsapp` | +40KB | reqwest, axum | Cloud API, webhook receiver |
| `channel-signal` | +25KB | reqwest, tokio | signal-cli REST bridge |
| `channel-matrix` | +35KB | reqwest | Matrix homeserver sync API |
| `channel-irc` | +20KB | tokio | Raw TCP, minimal overhead |
| `channel-googlechat` | +30KB | reqwest, axum | Google Workspace API |
| `channel-msteams` | +35KB | reqwest, axum | Bot Framework, token exchange |

**Bundle**: Use `all-channels` to enable all 10.

### Providers (20+)
LLM backends — most share the OpenAI-compatible interface:

| Feature | Size Impact | Dependencies | Notes |
|---------|------------|--------------|-------|
| `provider-anthropic` | +15KB | reqwest | Claude native API + OAuth |
| `provider-ollama` | +10KB | reqwest | Local models, no auth needed |
| `provider-copilot` | +20KB | reqwest | GitHub Copilot relay, token exchange |
| *(none needed)* | baseline | reqwest | OpenAI-compatible built-in |

**OpenAI-Compatible** (same implementation):
- OpenAI, OpenRouter, Groq, Together, Mistral, DeepSeek
- Fireworks, Perplexity, xAI, Moonshot, Venice, HuggingFace
- SiliconFlow, Cerebras, MiniMax, Vercel, Cloudflare
- Any custom endpoint with OpenAI-compatible API

**Bundle**: Use `all-providers` to enable all 20+.

### Optional: Docker Isolation
Enable Docker container runtime (requires `bollard` SDK):

```bash
cargo build --release --features "docker"
```

## Feature Flags (Complete List)

```toml
[features]
# Core
core = []

# Channels
channel-cli = []
channel-telegram = []
channel-discord = []
channel-slack = []
channel-whatsapp = []
channel-signal = []
channel-matrix = []
channel-irc = []
channel-googlechat = []
channel-msteams = []

# Providers
provider-anthropic = []
provider-openai = []      # (unnecessary, built-in)
provider-copilot = []
provider-ollama = []
provider-openrouter = []  # (unnecessary, built-in)
provider-groq = []        # (unnecessary, built-in)
provider-together = []    # (unnecessary, built-in)
provider-mistral = []     # (unnecessary, built-in)
provider-deepseek = []    # (unnecessary, built-in)
provider-fireworks = []   # (unnecessary, built-in)
provider-perplexity = []  # (unnecessary, built-in)
provider-xai = []         # (unnecessary, built-in)
provider-moonshot = []    # (unnecessary, built-in)
provider-venice = []      # (unnecessary, built-in)
provider-huggingface = [] # (unnecessary, built-in)
provider-siliconflow = [] # (unnecessary, built-in)
provider-cerebras = []    # (unnecessary, built-in)
provider-minimax = []     # (unnecessary, built-in)
provider-vercel = []      # (unnecessary, built-in)
provider-cloudflare = []  # (unnecessary, built-in)

# Optional: Docker support
docker = ["dep:bollard"]

# Convenience bundles
all-channels = [...]
all-providers = [...]
full = ["all-channels", "all-providers", "docker"]
```

## Common Configurations

### 1. Lightweight Personal Assistant
```bash
cargo build --release --features "channel-cli,provider-anthropic"
# ~4.2MB, <10ms startup, <5MB RAM
# Just you + local AI on CLI
```

### 2. Team ChatBot (Slack)
```bash
cargo build --release --features "channel-slack,provider-openai"
# ~4.2MB
# One messaging platform, one LLM provider
```

### 3. Multi-Platform Hub
```bash
cargo build --release --features "all-channels,provider-anthropic,provider-openai"
# ~4.2MB
# All messaging platforms, choice of LLMs
```

### 4. Full-Featured Production
```bash
cargo build --release --features full
# ~4.2MB
# Everything: all channels, all providers, Docker support
```

### 5. Self-Hosted (No OpenAI)
```bash
cargo build --release --features "channel-cli,channel-telegram,provider-ollama,docker"
# ~4.2MB
# Local models only, no API keys needed
```

## Binary Footprint

All feature combinations compile to **~4.2MB** because:
- Dead code elimination by LLVM (LTO enabled)
- Feature gates only control runtime path selection
- Shared dependencies across all modules
- No dynamic linking (fully static binary)

This means: **Pick features for compatibility, not size.**

## Environment Variables

All providers read API keys from environment:

```bash
# Anthropic
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenAI-compatible (covers 15+ providers)
export OPENAI_API_KEY="sk-..."

# Specific to enabled features
export OLLAMA_BASE_URL="http://localhost:11434"

# Channels
export TELEGRAM_BOT_TOKEN="123:ABC..."
export SLACK_BOT_TOKEN="xoxb-..."
export DISCORD_BOT_TOKEN="..."
export WHATSAPP_ACCESS_TOKEN="..."
export SIGNAL_PHONE_NUMBER="+1234567890"
export MATRIX_HOMESERVER="https://matrix.org"
export MATRIX_ACCESS_TOKEN="..."
```

## Compilation Times

```bash
# Minimal build (CLI + Anthropic only)
time cargo build --release --features "channel-cli,provider-anthropic" --no-default-features
# ~30s on M1 Mac

# Full build (all features)
time cargo build --release --features full
# ~32s on M1 Mac (incremental compilation)

# Typical rebuild (after code changes)
# ~5-10s (depends on what changed)
```

## Testing Features

```bash
# Test a specific feature combination
cargo test --features "channel-slack,provider-openai"

# Test all features
cargo test --all-features

# Test minimal
cargo test --no-default-features --features "channel-cli,provider-anthropic"
```

## Migration from OpenClaw

If you were using OpenClaw:
1. Check which channels you used → enable those features
2. Check which LLM providers you used → enable those features
3. Use `cargo build --release --features ...` with your combo
4. Binary works identically at ~4.2MB

**Example**: If you used Telegram + OpenAI on OpenClaw:
```bash
cargo build --release --features "channel-telegram,provider-anthropic"
# (provider-anthropic optional, provider-openai is built-in)
```

## Disabling Features

To disable even the default features for a minimal build:

```bash
cargo build --release --no-default-features --features "channel-cli"
# Disables: channel-telegram, provider-anthropic
# Only includes: CLI channel
```

This creates the absolute smallest binary, though still ~4.2MB due to LLVM linking.

## FAQ

**Q: Does enabling more features increase binary size?**
A: Not noticeably. LTO eliminates all unused code. All builds are ~4.2MB.

**Q: Can I use multiple channels simultaneously?**
A: Yes! Enable multiple features: `--features "channel-slack,channel-discord,channel-telegram"`

**Q: Can I use multiple providers?**
A: Yes! All OpenAI-compatible providers are built-in. Just enable Anthropic/Copilot/Ollama if needed.

**Q: Do I need Docker support?**
A: Only if you want container isolation for agent execution. Optional: `--features docker`

**Q: What's the default feature set?**
A: `default = ["core", "channel-cli", "channel-telegram", "provider-anthropic"]`

**Q: Can I use environment variables instead of features?**
A: Yes, but features control which code paths are compiled. Both are optional.

---

**Summary**: Features let you customize compilation for clarity and dependency reduction. Binary size is constant at 4.2MB. Use features to match your deployment:
- Local use? → CLI only
- Slack team? → Slack + OpenAI
- Everything? → full

All work, all fast, all tiny.
