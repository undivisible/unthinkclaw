//! Telegram channel for aclaw
//! Receive/send messages via Telegram bot (polling mode for simplicity)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::traits::{Channel, IncomingMessage, OutgoingMessage};

#[derive(Clone)]
pub struct TelegramChannel {
    bot_token: String,
    chat_id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TelegramResponse {
    ok: bool,
    result: Option<Vec<Update>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    message_id: i64,
    chat: Chat,
    text: Option<String>,
    from: User,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Chat {
    id: i64,
    #[serde(default)]
    is_group: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct User {
    id: i64,
    first_name: Option<String>,
    username: Option<String>,
}

impl TelegramChannel {
    pub fn new(bot_token: String, chat_id: i64) -> Self {
        Self { bot_token, chat_id }
    }

    /// Get updates (polling)
    async fn get_updates(&self, offset: i64) -> anyhow::Result<Vec<Update>> {
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates?offset={}&limit=100&timeout=30",
            self.bot_token, offset
        );

        match reqwest::get(&url).await {
            Ok(resp) => {
                if let Ok(data) = resp.json::<TelegramResponse>().await {
                    Ok(data.result.unwrap_or_default())
                } else {
                    Ok(Vec::new())
                }
            }
            Err(_) => Ok(Vec::new()),
        }
    }

    /// Send message to Telegram
    async fn send_message(&self, text: &str) -> anyhow::Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let _resp = reqwest::Client::new()
            .post(&url)
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "text": text,
            }))
            .send()
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&mut self) -> anyhow::Result<mpsc::Receiver<IncomingMessage>> {
        let (tx, rx) = mpsc::channel(100);
        let channel = self.clone();

        tokio::spawn(async move {
            let mut offset = 0;
            loop {
                if let Ok(updates) = channel.get_updates(offset).await {
                    for update in updates {
                        if let Some(msg) = &update.message {
                            if let Some(text) = &msg.text {
                                let incoming = IncomingMessage {
                                    id: msg.message_id.to_string(),
                                    sender_id: msg.from.id.to_string(),
                                    sender_name: msg
                                        .from
                                        .username
                                        .clone()
                                        .or_else(|| msg.from.first_name.clone()),
                                    chat_id: msg.chat.id.to_string(),
                                    text: text.clone(),
                                    is_group: msg.chat.is_group,
                                    reply_to: None,
                                    timestamp: chrono::Utc::now(),
                                };
                                let _ = tx.send(incoming).await;
                            }
                        }
                        offset = update.update_id + 1;
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });

        Ok(rx)
    }

    async fn send(&self, message: OutgoingMessage) -> anyhow::Result<()> {
        self.send_message(&message.text).await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
