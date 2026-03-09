//! unthinkclaw — Lightweight agent runtime CLI
//! Successor to OpenClaw. Best-of-breed from ZeroClaw, NanoClaw, HiClaw.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};

use unthinkclaw::agent::AgentRunner;
#[cfg(feature = "channel-cli")]
use unthinkclaw::channels::cli::CliChannel;
#[cfg(feature = "channel-telegram")]
use unthinkclaw::channels::telegram::TelegramChannel;
use unthinkclaw::channels::Channel as _;
#[cfg(feature = "channel-discord")]
use unthinkclaw::channels::discord::DiscordChannel;
use unthinkclaw::config::Config;
use unthinkclaw::cron_scheduler::CronScheduler;
use unthinkclaw::gateway;
use unthinkclaw::heartbeat::{self, HeartbeatConfig};
use unthinkclaw::memory::search::{MemorySearchTool, MemoryGetTool};
use unthinkclaw::memory::sqlite::SqliteMemory;
use unthinkclaw::memory::MemoryBackend;
use unthinkclaw::prompt;
use unthinkclaw::skills;
#[cfg(feature = "provider-anthropic")]
use unthinkclaw::providers::anthropic::AnthropicProvider;
#[cfg(feature = "provider-ollama")]
use unthinkclaw::providers::ollama::OllamaProvider;
use unthinkclaw::providers::openai_compat::OpenAiCompatProvider;
use unthinkclaw::providers::Provider;
use unthinkclaw::tools::file_ops::{FileReadTool, FileWriteTool};
use unthinkclaw::tools::shell::ShellTool;
use unthinkclaw::tools::Tool;

#[derive(Parser)]
#[command(name = "unthinkclaw", about = "Lightweight agent runtime — unthink everything", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start interactive agent chat
    Chat {
        /// Configuration file path
        #[arg(short, long, default_value = "unthinkclaw.json")]
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
        #[arg(short, long, default_value = "unthinkclaw.json")]
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
        #[arg(short, long, default_value = "unthinkclaw.json")]
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

    /// Manage cron jobs
    Cron {
        #[command(subcommand)]
        action: CronAction,

        /// Workspace directory
        #[arg(short, long)]
        workspace: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum CronAction {
    /// Add a new cron job
    Add {
        /// Job name
        #[arg(short, long)]
        name: String,

        /// Cron expression (e.g. "0 0 9 * * * *")
        #[arg(short, long)]
        schedule: String,

        /// Task prompt text
        #[arg(short, long)]
        task: String,

        /// Channel (default: cli)
        #[arg(long, default_value = "cli")]
        channel: String,

        /// Model override
        #[arg(long, default_value = "")]
        model: String,
    },

    /// List all cron jobs
    List,

    /// Remove a cron job by ID or name
    Remove {
        /// Job ID or name
        id_or_name: String,
    },

    /// Enable a cron job
    Enable {
        /// Job ID or name
        id_or_name: String,
    },

    /// Disable a cron job
    Disable {
        /// Job ID or name
        id_or_name: String,
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
                &workspace.join(".unthinkclaw/memory.db").to_string_lossy(),
            )?);

            // Build system prompt from workspace context files
            let system_prompt = prompt::build_system_prompt(&workspace);

            // Discover skills
            let discovered_skills = skills::discover_skills();
            if !discovered_skills.is_empty() {
                tracing::info!("Discovered {} skills", discovered_skills.len());
            }

            // Register tools (including memory search/get)
            let mut tools: Vec<Arc<dyn Tool>> = vec![
                Arc::new(ShellTool::new(workspace.clone())),           // exec
                Arc::new(FileReadTool::new(workspace.clone())),        // Read
                Arc::new(FileWriteTool::new(workspace.clone())),       // Write
                Arc::new(unthinkclaw::tools::edit::EditTool::new(workspace.clone())), // Edit
                Arc::new(MemorySearchTool::new(workspace.clone())),    // memory_search
                Arc::new(MemoryGetTool::new(workspace.clone())),       // memory_get
                Arc::new(unthinkclaw::tools::web_search::WebSearchTool::new()),  // web_search
                Arc::new(unthinkclaw::tools::web_fetch::WebFetchTool::new()),    // web_fetch
                Arc::new(unthinkclaw::tools::session::ListModelsTool::new()),    // list_models
                Arc::new(unthinkclaw::tools::dynamic::CreateToolTool::new()),    // create_tool
                Arc::new(unthinkclaw::tools::dynamic::ListCustomToolsTool::new()), // list_custom_tools
                Arc::new(unthinkclaw::tools::browser::BrowserTool::new()),       // browser (agent-browser)
                Arc::new(unthinkclaw::tools::mcp::McpTool::new()),               // mcp (Codex MCP client)
            ];

            // Load any previously created dynamic tools
            let dynamic_tools = unthinkclaw::tools::dynamic::DynamicTool::load_all();
            let dynamic_count = dynamic_tools.len();
            for dt in dynamic_tools {
                tools.push(Arc::new(dt));
            }
            if dynamic_count > 0 {
                println!("   Loaded {} custom tool(s)", dynamic_count);
            }

            let mut runner = AgentRunner::new(provider, tools, memory.clone(), &system_prompt, model)
                .with_workspace(workspace.clone())
                .with_skills(discovered_skills.clone());

            // Start cron scheduler background task
            let cron_db_path = workspace.join(".unthinkclaw/cron.db");
            if let Ok(cron_sched) = CronScheduler::new(&cron_db_path.to_string_lossy()) {
                let cron_sched = Arc::new(cron_sched);
                let (_cron_rx, _cron_shutdown) = unthinkclaw::cron_scheduler::start_cron_ticker(cron_sched);
                // Due jobs from cron_rx would be handled here in a full implementation
                // For now, the ticker runs and logs due jobs
            }

            match channel.as_str() {
                #[cfg(feature = "channel-cli")]
                "cli" => {
                    println!("unthinkclaw v{} — {} via {}", env!("CARGO_PKG_VERSION"), cfg.model, cfg.provider.name);
                    println!("   Workspace: {}", workspace.display());
                    println!("   Channel: CLI");
                    println!("   Type /quit to exit\n");

                    // Start heartbeat background task
                    let heartbeat_cfg = HeartbeatConfig {
                        workspace: workspace.clone(),
                        ..Default::default()
                    };
                    let (hb_tx, hb_rx) = tokio::sync::mpsc::channel(16);
                    let _heartbeat_handle = heartbeat::start_heartbeat(heartbeat_cfg, hb_tx);

                    let mut ch = CliChannel::new();
                    runner.run_with_extra_rx(&mut ch, hb_rx).await?;
                }
                #[cfg(feature = "channel-telegram")]
                "telegram" => {
                    let token = telegram_token.ok_or_else(|| anyhow::anyhow!("--telegram-token required"))?;
                    let chat_id = telegram_chat_id.ok_or_else(|| anyhow::anyhow!("--telegram-chat-id required"))?;

                    let tg = TelegramChannel::new(token.clone(), chat_id);
                    let tg_arc = Arc::new(tg.clone());

                    // Add late-binding tools that need references
                    runner.add_tool(Arc::new(unthinkclaw::tools::message::MessageTool::new(tg_arc.clone())));

                    // Wrap runner in Arc for session_status
                    let runner = Arc::new(runner);
                    // Note: session_status needs Arc<AgentRunner> but AgentRunner isn't Clone-able
                    // For now, session_status is available via /status command and list_models tool

                    println!("unthinkclaw — {} via Telegram", cfg.model);
                    println!("   Chat ID: {}", chat_id);
                    println!("   Tools: {}", runner.list_tools().join(", "));
                    println!("   Listening for messages...");
                    let mut ch = TelegramChannel::new(token, chat_id);
                    let mut rx = ch.start().await?;

                    while let Some(msg) = rx.recv().await {
                        let text = msg.text.trim();

                        // Handle slash commands
                        if text.starts_with('/') {
                            let parts: Vec<&str> = text.splitn(2, ' ').collect();
                            let cmd = parts[0].to_lowercase();
                            let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

                            match cmd.as_str() {
                                "/stop" | "/cancel" => {
                                    let _ = tg.send_message("⛔ Stopped.").await;
                                    continue;
                                }
                                "/help" => {
                                    let _ = tg.send_message(
                                        "🐾 *unthinkclaw commands:*\n\n\
                                        /stop — Stop current operation (saves tokens!)\n\
                                        /help — Show this message\n\
                                        /model — Show current model\n\
                                        /model <name> — Switch model\n\
                                        /models — List available models\n\
                                        /tools — List available tools\n\
                                        /status — Bot status\n\
                                        /cost — API usage & spending\n\
                                        /reset — Clear conversation history\n\n\
                                        Everything else is sent to the AI."
                                    ).await;
                                    continue;
                                }
                                "/model" | "/model@unthinkclaw_bot" => {
                                    if arg.is_empty() {
                                        let _ = tg.send_message(&format!(
                                            "Current model: `{}`\n\nUse `/model <name>` to switch.\nUse `/models` for available options.",
                                            runner.get_model()
                                        )).await;
                                    } else {
                                        runner.set_model(arg);
                                        let _ = tg.send_message(&format!("✅ Model switched to: `{}`", arg)).await;
                                        tracing::info!("Model switched to: {}", arg);
                                    }
                                    continue;
                                }
                                "/models" => {
                                    let _ = tg.send_message(
                                        "📋 *Available models:*\n\n\
                                        `claude-sonnet-4-5` — Fast, smart (default)\n\
                                        `claude-opus-4` — Most capable\n\
                                        `claude-haiku-3-5` — Fastest, cheapest\n\n\
                                        Switch with: `/model claude-opus-4`"
                                    ).await;
                                    continue;
                                }
                                "/tools" => {
                                    let tool_list = runner.list_tools();
                                    let formatted = tool_list.iter()
                                        .map(|t| format!("• `{}`", t))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    let _ = tg.send_message(&format!(
                                        "🔧 *Available tools ({}):\n\n{}*",
                                        tool_list.len(), formatted
                                    )).await;
                                    continue;
                                }
                                "/status" => {
                                    let _ = tg.send_message(&format!(
                                        "🐾 *unthinkclaw status:*\n\n\
                                        Model: `{}`\n\
                                        Tools: {}\n\
                                        Skills: {}\n\
                                        Channel: Telegram\n\
                                        PID: {}",
                                        runner.get_model(),
                                        runner.list_tools().len(),
                                        discovered_skills.len(),
                                        std::process::id(),
                                    )).await;
                                    continue;
                                }
                                "/reset" => {
                                    // Clear conversation history from SQLite
                                    let _ = memory.forget("chat", &format!("conv_{}", msg.chat_id)).await;
                                    let _ = tg.send_message("🗑 Conversation history cleared.").await;
                                    continue;
                                }
                                "/cost" => {
                                    let summary = runner.get_cost_summary().await;
                                    let mut by_model: Vec<_> = summary.by_model.iter().collect();
                                    by_model.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
                                    
                                    let model_breakdown = if by_model.is_empty() {
                                        "No usage yet.".to_string()
                                    } else {
                                        by_model.iter()
                                            .map(|(model, cost)| format!("  • {}: ${:.4}", model, cost))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    };
                                    
                                    let _ = tg.send_message(&format!(
                                        "💰 *Cost Summary:*\n\n\
                                        Total: ${:.4}\n\
                                        Tokens: {}\n\
                                        Calls: {}\n\n\
                                        By model:\n{}",
                                        summary.total_cost,
                                        summary.total_tokens,
                                        summary.call_count,
                                        model_breakdown,
                                    )).await;
                                    continue;
                                }
                                "/start" => {
                                    let _ = tg.send_message(
                                        "🐾 *unthinkclaw* — AI assistant\n\n\
                                        Just type a message to chat.\n\
                                        Use /help for commands.\n\
                                        Use /tools to see what I can do."
                                    ).await;
                                    continue;
                                }
                                _ => {
                                    // Unknown command — pass to AI
                                }
                            }
                        }

                        // Send "thinking..." progress message
                        let _ = tg.send_typing().await;
                        let progress_msg_id = tg.send_message("⏳").await.unwrap_or(0);

                        // Create progress channel
                        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(32);

                        // Spawn progress update task
                        let tg_progress = tg.clone();
                        let progress_task = tokio::spawn(async move {
                            while let Some(update) = progress_rx.recv().await {
                                use unthinkclaw::agent::loop_runner::ProgressUpdate;
                                let status_text = match update {
                                    ProgressUpdate::Thinking => "thinking...".to_string(),
                                    ProgressUpdate::ToolCall { name, round } => {
                                        // Descriptive tool names with emoji
                                        let display = match name.as_str() {
                                            "exec" => "running shell command",
                                            "Read" => "reading file",
                                            "Write" => "writing file",
                                            "Edit" => "editing file",
                                            "web_search" => "searching web",
                                            "web_fetch" => "fetching webpage",
                                            "memory_search" => "searching memory",
                                            "browser" => "browsing web",
                                            "create_tool" => "creating custom tool",
                                            _ => &name,
                                        };
                                        format!("🔧 {} (round {})", display, round)
                                    }
                                    ProgressUpdate::Processing { round, tool_count } => {
                                        if round == 0 || tool_count == 0 {
                                            break;
                                        }
                                        format!("processing... round {} ({} tools)", round, tool_count)
                                    }
                                };
                                
                                if progress_msg_id > 0 {
                                    let _ = tg_progress.edit_message(progress_msg_id, &status_text).await;
                                }
                            }
                        });

                        // Process message
                        match runner.handle_message_pub(&msg, Some(&progress_tx)).await {
                            Ok(response) => {
                                let _ = progress_tx.send(unthinkclaw::agent::loop_runner::ProgressUpdate::Processing {
                                    round: 0,
                                    tool_count: 0,
                                }).await;
                                drop(progress_tx);
                                let _ = progress_task.await;
                                
                                // Delete progress message
                                if progress_msg_id > 0 {
                                    let _ = tg.delete_message(progress_msg_id).await;
                                }
                                // Send final response
                                let _ = tg.send_message(&response).await;
                            }
                            Err(e) => {
                                drop(progress_tx);
                                let _ = progress_task.await;
                                
                                if progress_msg_id > 0 {
                                    let _ = tg.edit_message(progress_msg_id, &format!("❌ {}", e)).await;
                                }
                            }
                        }
                    }
                }
                #[cfg(feature = "channel-discord")]
                "discord" => {
                    let token = _discord_token.ok_or_else(|| anyhow::anyhow!("--discord-token required"))?;
                    let channel_id =
                        _discord_channel_id.ok_or_else(|| anyhow::anyhow!("--discord-channel-id required"))?;

                    println!("unthinkclaw — {} via Discord", cfg.model);
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
            println!("unthinkclaw Gateway — starting on {}", addr);
            println!("   API: http://{}/api/chat", addr);
            println!("   WebSocket: ws://{}/ws", addr);
            gateway::start_gateway(&addr).await?;
        }

        Commands::Status => {
            println!("unthinkclaw v{}", env!("CARGO_PKG_VERSION"));
            println!("Status: OK");
            println!("Commands: chat, ask, gateway, status, init, cron");
        }

        Commands::Init { provider, api_key } => {
            let mut cfg = Config::default_config();
            cfg.provider.name = provider;
            cfg.provider.api_key = api_key;
            let json = serde_json::to_string_pretty(&cfg)?;
            std::fs::write("unthinkclaw.json", &json)?;
            println!("Created unthinkclaw.json");
        }

        Commands::Cron { action, workspace } => {
            let workspace = workspace.unwrap_or_else(|| PathBuf::from("."));
            let db_path = workspace.join(".unthinkclaw/cron.db");
            let scheduler = CronScheduler::new(&db_path.to_string_lossy())?;

            match action {
                CronAction::Add { name, schedule, task, channel, model } => {
                    let id = scheduler.add(&name, &schedule, &task, &channel, &model)?;
                    println!("Added cron job: {} (id: {})", name, id);
                }
                CronAction::List => {
                    let jobs = scheduler.list()?;
                    if jobs.is_empty() {
                        println!("No cron jobs configured.");
                    } else {
                        for job in &jobs {
                            println!(
                                "{} [{}] {} — \"{}\" (next: {})",
                                if job.enabled { "+" } else { "-" },
                                job.name,
                                job.schedule,
                                job.task,
                                job.next_run.as_deref().unwrap_or("none"),
                            );
                        }
                    }
                }
                CronAction::Remove { id_or_name } => {
                    if scheduler.remove(&id_or_name)? {
                        println!("Removed: {}", id_or_name);
                    } else {
                        println!("Not found: {}", id_or_name);
                    }
                }
                CronAction::Enable { id_or_name } => {
                    if scheduler.enable(&id_or_name)? {
                        println!("Enabled: {}", id_or_name);
                    } else {
                        println!("Not found: {}", id_or_name);
                    }
                }
                CronAction::Disable { id_or_name } => {
                    if scheduler.disable(&id_or_name)? {
                        println!("Disabled: {}", id_or_name);
                    } else {
                        println!("Not found: {}", id_or_name);
                    }
                }
            }
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
            if let Ok(_provider) = unthinkclaw::providers::anthropic::AnthropicProvider::from_env_or_oauth() {
                // Fallback to Claude.dev credentials file
                let _ = _provider; // Just checking it exists
                if let Ok((token, _, _)) = unthinkclaw::providers::oauth::load_oauth_token_from_file() {
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
            if let Ok(p) = unthinkclaw::providers::copilot::CopilotProvider::from_openclaw() {
                Arc::new(p)
            } else {
                Arc::new(unthinkclaw::providers::copilot::CopilotProvider::new(&api_key))
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
