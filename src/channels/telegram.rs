//! Telegram channel — polling mode with progress feedback via message editing.
//! - Sends "thinking..." message immediately
//! - Edits it with tool call progress
//! - Deletes progress and sends final clean message

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use super::traits::{Channel, IncomingMessage, OutgoingMessage};

#[derive(Clone)]
pub struct TelegramChannel {
    bot_token: String,
    chat_id: i64,
    client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
struct TelegramResponse {
    ok: bool,
    result: Option<Vec<Update>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SendResult {
    ok: bool,
    result: Option<SentMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SentMessage {
    message_id: i64,
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
    from: Option<User>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Chat {
    id: i64,
    #[serde(rename = "type", default)]
    chat_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct User {
    id: i64,
    first_name: Option<String>,
    username: Option<String>,
}

impl TelegramChannel {
    pub fn new(bot_token: String, chat_id: i64) -> Self {
        Self {
            bot_token,
            chat_id,
            client: reqwest::Client::new(),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.bot_token, method)
    }

    /// Send a message and return its message_id
    pub async fn send_message(&self, text: &str) -> anyhow::Result<i64> {
        let resp = self.client
            .post(self.api_url("sendMessage"))
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "text": text,
                "parse_mode": "Markdown",
            }))
            .send()
            .await?;

        // If Markdown fails, retry without parse_mode
        let body: Value = resp.json().await?;
        if body["ok"].as_bool() == Some(true) {
            return Ok(body["result"]["message_id"].as_i64().unwrap_or(0));
        }

        // Retry without markdown
        let resp = self.client
            .post(self.api_url("sendMessage"))
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "text": text,
            }))
            .send()
            .await?;
        let body: Value = resp.json().await?;
        Ok(body["result"]["message_id"].as_i64().unwrap_or(0))
    }

    /// Edit an existing message
    pub async fn edit_message(&self, message_id: i64, text: &str) -> anyhow::Result<()> {
        let _ = self.client
            .post(self.api_url("editMessageText"))
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "message_id": message_id,
                "text": text,
            }))
            .send()
            .await?;
        Ok(())
    }

    /// Delete a message
    pub async fn delete_message(&self, message_id: i64) -> anyhow::Result<()> {
        let _ = self.client
            .post(self.api_url("deleteMessage"))
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "message_id": message_id,
            }))
            .send()
            .await?;
        Ok(())
    }

    /// Send typing indicator
    pub async fn send_typing(&self) -> anyhow::Result<()> {
        let _ = self.client
            .post(self.api_url("sendChatAction"))
            .json(&serde_json::json!({
                "chat_id": self.chat_id,
                "action": "typing",
            }))
            .send()
            .await?;
        Ok(())
    }

    /// Get updates (long polling)
    async fn get_updates(&self, offset: i64) -> anyhow::Result<Vec<Update>> {
        let url = format!(
            "{}?offset={}&limit=100&timeout=30",
            self.api_url("getUpdates"),
            offset
        );

        match self.client.get(&url).send().await {
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
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&mut self) -> anyhow::Result<mpsc::Receiver<IncomingMessage>> {
        let (tx, rx) = mpsc::channel(100);
        let bot_token = self.bot_token.clone();
        let chat_id = self.chat_id;
        let client = self.client.clone();

        tokio::spawn(async move {
            let ch = TelegramChannel { bot_token, chat_id, client };
            let mut offset = 0;
            loop {
                if let Ok(updates) = ch.get_updates(offset).await {
                    for update in updates {
                        if let Some(msg) = &update.message {
                            if let Some(text) = &msg.text {
                                let from = msg.from.as_ref();
                                let is_group = msg.chat.chat_type.as_deref()
                                    .map(|t| t == "group" || t == "supergroup")
                                    .unwrap_or(false);

                                let incoming = IncomingMessage {
                                    id: msg.message_id.to_string(),
                                    sender_id: from.map(|u| u.id.to_string()).unwrap_or_default(),
                                    sender_name: from.and_then(|u| {
                                        u.username.clone().or_else(|| u.first_name.clone())
                                    }),
                                    chat_id: msg.chat.id.to_string(),
                                    text: text.clone(),
                                    is_group,
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
        let _ = self.send_message(&message.text).await?;
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
