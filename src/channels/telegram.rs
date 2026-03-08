//! Telegram channel — polling mode with progress feedback via message editing.
//! - Sends "thinking..." message immediately
//! - Edits it with tool call progress
//! - Deletes progress and sends final clean message

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use super::traits::{Channel, IncomingMessage, OutgoingMessage};

/// Telegram message length limit
const TELEGRAM_MAX_LEN: usize = 4096;

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
#[allow(dead_code)]
struct SendResult {
    ok: bool,
    result: Option<SentMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
struct SentMessage {
    message_id: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Voice {
    file_id: String,
    #[serde(default)]
    file_unique_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Audio {
    file_id: String,
    #[serde(default)]
    file_unique_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Location {
    latitude: f64,
    longitude: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Sticker {
    file_id: String,
    #[serde(default)]
    file_unique_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    message_id: i64,
    chat: Chat,
    text: Option<String>,
    from: Option<User>,
    voice: Option<Voice>,
    audio: Option<Audio>,
    location: Option<Location>,
    sticker: Option<Sticker>,
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

/// Sanitize Markdown for Telegram's strict parser
fn sanitize_markdown(text: &str) -> String {
    let mut result = String::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut in_code_block = false;
    let mut in_table = false;
    
    for line in lines {
        let trimmed = line.trim();
        
        // Track code blocks
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            // Ensure code blocks have language specifiers
            if in_code_block && trimmed == "```" {
                result.push_str("```text\n");
                continue;
            }
        }
        
        // Inside code blocks, pass through unchanged
        if in_code_block {
            result.push_str(line);
            result.push('\n');
            continue;
        }
        
        // Detect table rows (contain pipes, not inside code)
        if trimmed.contains('|') && !trimmed.is_empty() {
            let cells: Vec<&str> = trimmed.split('|')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            
            // Skip separator rows (only dashes/spaces)
            if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ' ')) {
                continue;
            }
            
            // Convert to bullet list
            if !cells.is_empty() {
                in_table = true;
                result.push_str(&format!("• {}\n", cells.join(" | ")));
                continue;
            }
        } else if in_table && !trimmed.is_empty() {
            in_table = false;
        }
        
        // Pass through other lines unchanged
        result.push_str(line);
        result.push('\n');
    }
    
    result.trim_end().to_string()
}

/// Chunk message into pieces under max_len, splitting at paragraph/sentence boundaries
fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    
    let mut chunks = Vec::new();
    let mut current = String::new();
    
    // First, try splitting by paragraphs (double newline)
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    
    for para in paragraphs {
        // If adding this paragraph would exceed limit
        if current.len() + para.len() + 2 > max_len {
            // If current is not empty, save it
            if !current.is_empty() {
                chunks.push(current.clone());
                current.clear();
            }
            
            // If the paragraph itself is too long, split by sentences
            if para.len() > max_len {
                let sentences: Vec<&str> = para.split(". ").collect();
                for (i, sent) in sentences.iter().enumerate() {
                    let sentence = if i < sentences.len() - 1 {
                        format!("{}. ", sent)
                    } else {
                        sent.to_string()
                    };
                    
                    if current.len() + sentence.len() > max_len {
                        if !current.is_empty() {
                            chunks.push(current.clone());
                            current.clear();
                        }
                        // If a single sentence is still too long, hard split
                        if sentence.len() > max_len {
                            for chunk in sentence.as_bytes().chunks(max_len) {
                                chunks.push(String::from_utf8_lossy(chunk).to_string());
                            }
                        } else {
                            current = sentence;
                        }
                    } else {
                        current.push_str(&sentence);
                    }
                }
            } else {
                current = para.to_string();
            }
        } else {
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(para);
        }
    }
    
    // Add remaining
    if !current.is_empty() {
        chunks.push(current);
    }
    
    chunks
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

    /// Transcribe voice/audio file using faster-whisper
    async fn transcribe_voice(&self, file_id: &str) -> anyhow::Result<String> {
        // Get file info from Telegram API
        let file_info_url = format!(
            "https://api.telegram.org/bot{}/getFile?file_id={}",
            self.bot_token, file_id
        );
        
        let resp = self.client.get(&file_info_url).send().await?;
        let body: Value = resp.json().await?;
        
        if body["ok"].as_bool() != Some(true) {
            return Ok(String::new());
        }
        
        let file_path = body["result"]["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No file_path in response"))?;
        
        // Download the file
        let download_url = format!(
            "https://api.telegram.org/file/bot{}/{}",
            self.bot_token, file_path
        );
        
        let file_resp = self.client.get(&download_url).send().await?;
        let file_bytes = file_resp.bytes().await?;
        
        // Create temp file
        let temp_path = format!("/tmp/voice_{}.ogg", uuid::Uuid::new_v4());
        tokio::fs::write(&temp_path, file_bytes).await?;
        
        // Call faster-whisper via Python (async)
        let output = tokio::process::Command::new("python3")
            .arg("-c")
            .arg(format!(
                r#"
import sys
from faster_whisper import WhisperModel
model = WhisperModel("tiny", device="cpu", compute_type="int8")
segments, _ = model.transcribe("{}", language="en")
text = " ".join([segment.text for segment in segments])
print(text)
"#,
                temp_path
            ))
            .output()
            .await?;
        
        // Clean up temp file
        let _ = tokio::fs::remove_file(&temp_path).await;
        
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_string())
        } else {
            Ok(String::new())
        }
    }

    /// Send a message and return its message_id (of the last chunk if split)
    pub async fn send_message(&self, text: &str) -> anyhow::Result<i64> {
        // Sanitize markdown
        let sanitized = sanitize_markdown(text);
        
        // Chunk if needed
        let chunks = chunk_message(&sanitized, TELEGRAM_MAX_LEN);
        
        let mut last_msg_id = 0;
        
        for (i, chunk) in chunks.iter().enumerate() {
            // Try with Markdown first
            let resp = self.client
                .post(self.api_url("sendMessage"))
                .json(&serde_json::json!({
                    "chat_id": self.chat_id,
                    "text": chunk,
                    "parse_mode": "Markdown",
                }))
                .send()
                .await?;

            let body: Value = resp.json().await?;
            
            if body["ok"].as_bool() == Some(true) {
                last_msg_id = body["result"]["message_id"].as_i64().unwrap_or(0);
            } else {
                // Markdown failed, retry without parse_mode
                let resp = self.client
                    .post(self.api_url("sendMessage"))
                    .json(&serde_json::json!({
                        "chat_id": self.chat_id,
                        "text": chunk,
                    }))
                    .send()
                    .await?;
                let body: Value = resp.json().await?;
                last_msg_id = body["result"]["message_id"].as_i64().unwrap_or(0);
            }
            
            // Add delay between chunks to avoid rate limiting
            if i < chunks.len() - 1 {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        
        Ok(last_msg_id)
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
                            let from = msg.from.as_ref();
                            let is_group = msg.chat.chat_type.as_deref()
                                .map(|t| t == "group" || t == "supergroup")
                                .unwrap_or(false);

                            // Determine message content based on message type
                            let text = if let Some(loc) = &msg.location {
                                // Location: format as coordinates with Google Maps link
                                format!("📍 Location: {}, {} (https://maps.google.com/?q={},{})", 
                                    loc.latitude, loc.longitude, loc.latitude, loc.longitude)
                            } else if let Some(_sticker) = &msg.sticker {
                                // Sticker: just note it was received
                                "🎨 Sticker received".to_string()
                            } else if let Some(voice) = &msg.voice {
                                // Voice: transcribe with faster-whisper
                                ch.transcribe_voice(&voice.file_id).await.unwrap_or_default()
                            } else if let Some(audio) = &msg.audio {
                                // Audio: transcribe with faster-whisper
                                ch.transcribe_voice(&audio.file_id).await.unwrap_or_default()
                            } else if let Some(text_content) = &msg.text {
                                // Regular text
                                text_content.clone()
                            } else {
                                // Unknown message type, skip
                                continue;
                            };

                            if text.is_empty() {
                                continue;
                            }

                            let incoming = IncomingMessage {
                                id: msg.message_id.to_string(),
                                sender_id: from.map(|u| u.id.to_string()).unwrap_or_default(),
                                sender_name: from.and_then(|u| {
                                    u.username.clone().or_else(|| u.first_name.clone())
                                }),
                                chat_id: msg.chat.id.to_string(),
                                text,
                                is_group,
                                reply_to: None,
                                timestamp: chrono::Utc::now(),
                            };
                            let _ = tx.send(incoming).await;
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
