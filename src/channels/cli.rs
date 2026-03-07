//! CLI channel — interactive terminal chat.

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::traits::*;

pub struct CliChannel;

impl CliChannel {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &str { "cli" }

    async fn start(&mut self) -> anyhow::Result<mpsc::Receiver<IncomingMessage>> {
        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            let stdin = tokio::io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            while let Ok(Some(line_input)) = lines.next_line().await {
                let line = line_input.trim().to_string();
                if line.is_empty() { continue; }
                if line == "/quit" || line == "/exit" { break; }

                let msg = IncomingMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    sender_id: "local".to_string(),
                    sender_name: Some("user".to_string()),
                    chat_id: "cli".to_string(),
                    text: line,
                    is_group: false,
                    reply_to: None,
                    timestamp: chrono::Utc::now(),
                };

                if tx.send(msg).await.is_err() { break; }
            }
        });

        Ok(rx)
    }

    async fn send(&self, message: OutgoingMessage) -> anyhow::Result<()> {
        println!("{}", message.text);
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
