//! Discord channel for aclaw
//! Simple text-based Discord bot integration

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::traits::{Channel, IncomingMessage, OutgoingMessage};

#[derive(Clone)]
pub struct DiscordChannel {
    bot_token: String,
    channel_id: String,
}

impl DiscordChannel {
    pub fn new(bot_token: String, channel_id: String) -> Self {
        Self {
            bot_token,
            channel_id,
        }
    }

    /// Send message to Discord
    async fn send_message(&self, text: &str) -> anyhow::Result<()> {
        let url = format!(
            "https://discordapp.com/api/channels/{}/messages",
            self.channel_id
        );
        let _resp = reqwest::Client::new()
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .json(&serde_json::json!({
                "content": text,
            }))
            .send()
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    async fn start(&mut self) -> anyhow::Result<mpsc::Receiver<IncomingMessage>> {
        let (_tx, rx) = mpsc::channel(100);

        // For Discord, we'd normally set up a websocket gateway
        // For now, return empty receiver (webhook-based would be easier)
        // User can POST to /webhook/{channel} via gateway

        Ok(rx)
    }

    async fn send(&self, message: OutgoingMessage) -> anyhow::Result<()> {
        self.send_message(&message.text).await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
