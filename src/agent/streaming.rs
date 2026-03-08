//! Streaming responses — chunked output for long-running tasks
//! Compatible with HTTP Server-Sent Events (SSE)

use serde::Serialize;
use tokio::sync::mpsc;

/// Streaming output chunk
#[derive(Debug, Clone, Serialize)]
pub struct StreamChunk {
    pub id: String,
    pub chunk: String,
    pub is_tool_use: bool,
    pub tool_name: Option<String>,
    pub index: usize,
}

/// Stream receiver — use for WebSocket/SSE responses
pub type StreamReceiver = mpsc::UnboundedReceiver<StreamChunk>;

/// Stream sender — used internally by agent loop
pub type StreamSender = mpsc::UnboundedSender<StreamChunk>;

/// Create a streaming channel
pub fn stream_channel(_id: &str) -> (StreamSender, StreamReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (tx, rx)
}

/// Collect all chunks into a final response
pub async fn collect_stream(mut rx: StreamReceiver) -> String {
    let mut output = String::new();
    while let Some(chunk) = rx.recv().await {
        output.push_str(&chunk.chunk);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stream_chunks() {
        let (tx, rx) = stream_channel("test");

        tokio::spawn(async move {
            for i in 0..3 {
                let chunk = StreamChunk {
                    id: "test".to_string(),
                    chunk: format!("chunk {}", i),
                    is_tool_use: false,
                    tool_name: None,
                    index: i,
                };
                let _ = tx.send(chunk);
            }
        });

        let output = collect_stream(rx).await;
        assert_eq!(output, "chunk 0chunk 1chunk 2");
    }
}
