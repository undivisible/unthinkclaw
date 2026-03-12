//! Configuration management.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: ProviderConfig,
    pub model: String,
    pub system_prompt: String,
    pub workspace: PathBuf,
    pub storage: StorageConfig,
    pub runtime: RuntimeConfig,
    pub hosting: HostingConfig,
    pub channel: ChannelConfig,
    pub gateway: GatewayConfig,
    pub policy: PolicyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub name: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub kind: String, // "native", "docker"
    pub docker_image: Option<String>,
    pub memory_limit_mb: Option<u64>,
    pub state_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub backend: String, // "surreal" | "sqlite" | "auto"
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HostingConfig {
    pub enabled: bool,
    pub tenant_root: PathBuf,
    pub session_timeout_minutes: u64,
    pub default_channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelConfig {
    pub kind: String, // "cli", "telegram", "discord", "websocket"
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub bind: String,
    pub auth_token: Option<String>,
    pub enable_admin_api: bool,
    pub request_body_limit_kb: usize,
    pub request_timeout_secs: u64,
    pub rate_limit_per_minute: usize,
    pub trusted_proxies: Vec<String>,
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicyConfig {
    pub allow_shell: bool,
    pub allow_dynamic_tools: bool,
    pub allow_plugin_shell: bool,
    pub allow_plugin_git: bool,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn default_config() -> Self {
        Self {
            provider: ProviderConfig {
                name: "anthropic".to_string(),
                api_key: None,
                base_url: None,
            },
            model: "claude-sonnet-4-5".to_string(),
            system_prompt: "You are a helpful AI assistant.".to_string(),
            workspace: PathBuf::from("."),
            storage: StorageConfig::default(),
            runtime: RuntimeConfig {
                kind: "native".to_string(),
                docker_image: None,
                memory_limit_mb: None,
                state_path: None,
            },
            hosting: HostingConfig::default(),
            channel: ChannelConfig {
                kind: "cli".to_string(),
                token: None,
            },
            gateway: GatewayConfig::default(),
            policy: PolicyConfig::default(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::default_config()
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: "anthropic".to_string(),
            api_key: None,
            base_url: None,
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            kind: "native".to_string(),
            docker_image: None,
            memory_limit_mb: None,
            state_path: None,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: "surreal".to_string(),
            root: PathBuf::from(".unthinkclaw"),
        }
    }
}

impl Default for HostingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tenant_root: PathBuf::from(".unthinkclaw/tenants"),
            session_timeout_minutes: 120,
            default_channel: "gateway".to_string(),
        }
    }
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            kind: "cli".to_string(),
            token: None,
        }
    }
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".to_string(),
            auth_token: None,
            enable_admin_api: false,
            request_body_limit_kb: 512,
            request_timeout_secs: 60,
            rate_limit_per_minute: 120,
            trusted_proxies: Vec::new(),
            allowed_origins: Vec::new(),
        }
    }
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            allow_shell: true,
            allow_dynamic_tools: true,
            allow_plugin_shell: false,
            allow_plugin_git: false,
        }
    }
}
