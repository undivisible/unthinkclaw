//! aclaw — Lightweight agent runtime CLI
//! Successor to OpenClaw. Best-of-breed from ZeroClaw, NanoClaw, HiClaw.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use aclaw::agent::AgentRunner;
#[cfg(feature = "channel-cli")]
use aclaw::channels::cli::CliChannel;
#[cfg(feature = "channel-telegram")]
use aclaw::channels::telegram::TelegramChannel;
#[cfg(feature = "channel-discord")]
use aclaw::channels::discord::DiscordChannel;
use aclaw::config::Config;
use aclaw::gateway;
use aclaw::memory::sqlite::SqliteMemory;
#[cfg(feature = "provider-anthropic")]
use aclaw::providers::anthropic::AnthropicProvider;
#[cfg(feature = "provider-ollama")]
use aclaw::providers::ollama::OllamaProvider;
use aclaw::providers::openai_compat::OpenAiCompatProvider;
use aclaw::providers::Provider;
use aclaw::tools::file_ops::{FileReadTool, FileWriteTool};
use aclaw::tools::shell::ShellTool;
use aclaw::tools::Tool;

#[derive(Parser)]
#[command(name = "aclaw", about = "Lightweight agent runtime — successor to OpenClaw", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start interactive agent chat
    Chat {
        /// Configuration file path
        #[arg(short, long, default_value = "aclaw.json")]
        config: String,

        /// Override the model
        #[arg(short, long)]
        model: Option<String>,

        /// Workspace directory
        #[arg(short, long)]
        workspace: Option<PathBuf>,

        /// Channel: cli, telegram, discord
        #[arg(long, default_value = "cli")]
        channel: String,

        /// Telegram bot token (required for --channel telegram)
        #[arg(long)]
        telegram_token: Option<String>,

        /// Telegram chat ID (required for --channel telegram)
        #[arg(long)]
        telegram_chat_id: Option<i64>,

        /// Discord bot token (required for --channel discord)
        #[arg(long)]
        discord_token: Option<String>,

        /// Discord channel ID (required for --channel discord)
        #[arg(long)]
        discord_channel_id: Option<String>,
    },

    /// Send a one-shot message
    Ask {
        /// The message to send
        message: String,

        /// Configuration file path
        #[arg(short, long, default_value = "aclaw.json")]
        config: String,

        /// Override the model
        #[arg(short, long)]
        model: Option<String>,
    },

    /// Start HTTP/WebSocket gateway
    Gateway {
        /// Listen address
        #[arg(short, long, default_value = "0.0.0.0:8080")]
        addr: String,

        /// Configuration file path
        #[arg(short, long, default_value = "aclaw.json")]
        config: String,
    },

    /// Show runtime status
    Status,

    /// Initialize configuration
    Init {
        /// Provider name (anthropic, openai, openrouter, ollama)
        #[arg(short, long, default_value = "anthropic")]
        provider: String,

        /// API key
        #[arg(short = 'k', long)]
        api_key: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Chat {
            config,
            model,
            workspace,
            channel,
            telegram_token,
            telegram_chat_id,
            discord_token: _discord_token,
            discord_channel_id: _discord_channel_id,
        } => {
            let cfg = load_config(&config);
            let model = model.unwrap_or(cfg.model.clone());
            let workspace = workspace.unwrap_or(cfg.workspace.clone());

            let provider = build_provider(&cfg);
            let memory = Arc::new(SqliteMemory::new(
                &workspace.join(".aclaw/memory.db").to_string_lossy(),
            )?);

            let tools: Vec<Arc<dyn Tool>> = vec![
                Arc::new(ShellTool::new(workspace.clone())),
                Arc::new(FileReadTool::new(workspace.clone())),
                Arc::new(FileWriteTool::new(workspace.clone())),
            ];

            let runner = AgentRunner::new(provider, tools, memory, &cfg.system_prompt, model);

            match channel.as_str() {
                #[cfg(feature = "channel-cli")]
                "cli" => {
                    println!("🚀 aclaw — {} via {}", cfg.model, cfg.provider.name);
                    println!("   Workspace: {}", workspace.display());
                    println!("   Channel: CLI");
                    println!("   Type /quit to exit\n");

                    let mut ch = CliChannel::new();
                    runner.run(&mut ch).await?;
                }
                #[cfg(feature = "channel-telegram")]
                "telegram" => {
                    let token = telegram_token.ok_or_else(|| anyhow::anyhow!("--telegram-token required"))?;
                    let chat_id = telegram_chat_id.ok_or_else(|| anyhow::anyhow!("--telegram-chat-id required"))?;

                    println!("🚀 aclaw — {} via Telegram", cfg.model);
                    println!("   Chat ID: {}", chat_id);
                    println!("   Listening for messages...");

                    let mut ch = TelegramChannel::new(token, chat_id);
                    runner.run(&mut ch).await?;
                }
                #[cfg(feature = "channel-discord")]
                "discord" => {
                    let token = _discord_token.ok_or_else(|| anyhow::anyhow!("--discord-token required"))?;
                    let channel_id =
                        _discord_channel_id.ok_or_else(|| anyhow::anyhow!("--discord-channel-id required"))?;

                    println!("🚀 aclaw — {} via Discord", cfg.model);
                    println!("   Channel ID: {}", channel_id);
                    println!("   Listening for messages...");

                    let mut ch = DiscordChannel::new(token, channel_id);
                    runner.run(&mut ch).await?;
                }
                other => {
                    anyhow::bail!("Unknown channel: {} (supported: cli, telegram, discord)", other);
                }
            }
        }

        Commands::Ask { message, config, model } => {
            let cfg = load_config(&config);
            let model = model.unwrap_or(cfg.model.clone());
            let provider = build_provider(&cfg);

            let response = provider.simple_chat(&message, &model).await?;
            println!("{}", response);
        }

        Commands::Gateway { addr, config } => {
            let _cfg = load_config(&config);
            println!("🌐 aclaw Gateway — starting on {}", addr);
            println!("   API: http://{}/api/chat", addr);
            println!("   WebSocket: ws://{}/ws", addr);
            gateway::start_gateway(&addr).await?;
        }

        Commands::Status => {
            println!("aclaw v{}", env!("CARGO_PKG_VERSION"));
            println!("Status: OK");
            println!("Commands: chat, ask, gateway, status, init");
        }

        Commands::Init { provider, api_key } => {
            let mut cfg = Config::default_config();
            cfg.provider.name = provider;
            cfg.provider.api_key = api_key;
            let json = serde_json::to_string_pretty(&cfg)?;
            std::fs::write("aclaw.json", &json)?;
            println!("✅ Created aclaw.json");
        }
    }

    Ok(())
}

fn load_config(path: &str) -> Config {
    Config::load(path).unwrap_or_else(|_| {
        tracing::warn!("Config not found at {}, using defaults", path);
        let mut cfg = Config::default_config();
        // Try env vars first, then OpenClaw auth-profiles, then Claude.dev credentials
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            cfg.provider.api_key = Some(key.clone());
            // OAuth tokens need compatible model names
            if key.contains("sk-ant-oat") {
                cfg.model = "claude-sonnet-4-5".to_string();
            }
        } else if let Ok(token) = resolve_openclaw_token("anthropic") {
            cfg.provider.api_key = Some(token);
            cfg.model = "claude-sonnet-4-5".to_string(); // OAuth-compatible model
        } 
        #[cfg(feature = "provider-anthropic")]
        {
            if let Ok(_provider) = aclaw::providers::anthropic::AnthropicProvider::from_env_or_oauth() {
                // Fallback to Claude.dev credentials file
                let _ = _provider; // Just checking it exists
                if let Ok((token, _, _)) = aclaw::providers::oauth::load_oauth_token_from_file() {
                    cfg.provider.api_key = Some(token);
                    cfg.model = "claude-sonnet-4-5".to_string();
                }
            }
        }
        
        if cfg.provider.api_key.is_none() {
            if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                cfg.provider.name = "openai".to_string();
                cfg.provider.api_key = Some(key);
            }
        }
        cfg
    })
}

/// Resolve token from OpenClaw's auth-profiles.json
fn resolve_openclaw_token(provider: &str) -> anyhow::Result<String> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("No home dir"))?;
    let auth_path = home.join(".openclaw/agents/main/agent/auth-profiles.json");
    
    if !auth_path.exists() {
        return Err(anyhow::anyhow!("No auth-profiles.json found"));
    }
    
    let content = std::fs::read_to_string(&auth_path)?;
    let data: serde_json::Value = serde_json::from_str(&content)?;
    
    // Try provider:default first
    let profile_key = format!("{}:default", provider);
    if let Some(profile) = data["profiles"][&profile_key].as_object() {
        // Check for token field (OAuth/token type)
        if let Some(token) = profile.get("token").and_then(|t| t.as_str()) {
            if !token.is_empty() {
                tracing::info!("Loaded {} token from OpenClaw auth-profiles", provider);
                return Ok(token.to_string());
            }
        }
        // Check for key field (API key type)
        if let Some(key) = profile.get("key").and_then(|k| k.as_str()) {
            if !key.is_empty() {
                tracing::info!("Loaded {} API key from OpenClaw auth-profiles", provider);
                return Ok(key.to_string());
            }
        }
    }
    
    // Try any profile for this provider
    if let Some(profiles) = data["profiles"].as_object() {
        for (key, value) in profiles {
            if let Some(p) = value["provider"].as_str() {
                if p == provider {
                    if let Some(token) = value["token"].as_str() {
                        if !token.is_empty() {
                            tracing::info!("Loaded {} token from OpenClaw profile {}", provider, key);
                            return Ok(token.to_string());
                        }
                    }
                    if let Some(key_val) = value["key"].as_str() {
                        if !key_val.is_empty() {
                            tracing::info!("Loaded {} key from OpenClaw profile {}", provider, key);
                            return Ok(key_val.to_string());
                        }
                    }
                }
            }
        }
    }
    
    Err(anyhow::anyhow!("No {} credentials in auth-profiles", provider))
}

fn build_provider(cfg: &Config) -> Arc<dyn Provider> {
    let api_key = cfg.provider.api_key.clone().unwrap_or_default();

    match cfg.provider.name.as_str() {
        #[cfg(feature = "provider-anthropic")]
        "anthropic" | "claude" => {
            let mut p = AnthropicProvider::new(&api_key);
            if let Some(url) = &cfg.provider.base_url {
                p = p.with_base_url(url);
            }
            Arc::new(p)
        }
        #[cfg(feature = "provider-copilot")]
        "github-copilot" | "copilot" => {
            if let Ok(p) = aclaw::providers::copilot::CopilotProvider::from_openclaw() {
                Arc::new(p)
            } else {
                Arc::new(aclaw::providers::copilot::CopilotProvider::new(&api_key))
            }
        }
        #[cfg(feature = "provider-ollama")]
        "ollama" => {
            let url = cfg.provider.base_url.clone().unwrap_or_else(|| "http://localhost:11434".into());
            Arc::new(OllamaProvider::new(url))
        }
        // All OpenAI-compatible providers (always available)
        "openai" => Arc::new(OpenAiCompatProvider::openai(&api_key)),
        "openrouter" => Arc::new(OpenAiCompatProvider::openrouter(&api_key)),
        "groq" => Arc::new(OpenAiCompatProvider::groq(&api_key)),
        "together" => Arc::new(OpenAiCompatProvider::together(&api_key)),
        "mistral" => Arc::new(OpenAiCompatProvider::mistral(&api_key)),
        "deepseek" => Arc::new(OpenAiCompatProvider::deepseek(&api_key)),
        "fireworks" => Arc::new(OpenAiCompatProvider::fireworks(&api_key)),
        "perplexity" => Arc::new(OpenAiCompatProvider::perplexity(&api_key)),
        "xai" | "grok" => Arc::new(OpenAiCompatProvider::xai(&api_key)),
        "moonshot" | "kimi" => Arc::new(OpenAiCompatProvider::moonshot(&api_key)),
        "venice" => Arc::new(OpenAiCompatProvider::venice(&api_key)),
        "huggingface" => Arc::new(OpenAiCompatProvider::huggingface(&api_key)),
        "siliconflow" => Arc::new(OpenAiCompatProvider::siliconflow(&api_key)),
        "cerebras" => Arc::new(OpenAiCompatProvider::cerebras(&api_key)),
        "minimax" => Arc::new(OpenAiCompatProvider::minimax(&api_key)),
        "vercel" => Arc::new(OpenAiCompatProvider::vercel(&api_key)),
        other => {
            let base = cfg.provider.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".into());
            Arc::new(OpenAiCompatProvider::new(&api_key, base, other))
        }
    }
}
