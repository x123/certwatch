//! Integration tests for the CertStream WebSocket client
//! 
//! These tests verify the client's connection, reconnection, and message
//! processing logic using fake WebSocket implementations.

use anyhow::Result;
use async_trait::async_trait;
use certwatch::{internal_metrics::Metrics, network::{CertStreamClient, WebSocketConnection}};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use async_channel;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

/// Fake WebSocket implementation for testing
#[derive(Debug)]
pub struct FakeWebSocket {
    messages: Vec<Option<Result<String, WsError>>>,
    current_index: Arc<Mutex<usize>>,
}

impl FakeWebSocket {
    /// Creates a new fake WebSocket with predefined text messages
    pub fn new_with_text_messages(messages: Vec<Option<Result<String, WsError>>>) -> Self {
        Self {
            messages,
            current_index: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl WebSocketConnection for FakeWebSocket {
    async fn read_message(&mut self) -> Option<Result<Message, WsError>> {
        let mut index = self.current_index.lock().unwrap();
        if *index >= self.messages.len() {
            return None;
        }
        let message_result = &self.messages[*index];
        *index += 1;

        match message_result {
            Some(Ok(text)) => Some(Ok(Message::Text(text.clone().into()))),
            Some(Err(_)) => Some(Err(WsError::ConnectionClosed)),
            None => None,
        }
    }
}

/// Test helper to run the client and signal completion.
async fn run_client_with_signal(
    client: CertStreamClient,
    connection: FakeWebSocket,
) -> oneshot::Receiver<()> {
    let (completion_tx, completion_rx) = oneshot::channel();
    tokio::spawn(async move {
        // We don't care about the result, just that it finishes.
        let _ = client.run_with_connection(Box::new(connection)).await;
        // Signal that the client has finished processing.
        let _ = completion_tx.send(());
    });
    completion_rx
}

#[tokio::test]
async fn test_certstream_client_basic_functionality() -> Result<()> {
    // 1. Arrange
    let sample_message = r#"{"data": ["test.example.com", "www.test.example.com"]}"#;
    let fake_ws = FakeWebSocket::new_with_text_messages(vec![
        Some(Ok(sample_message.to_string())),
        None, // Simulate connection close
    ]);
    let (tx, rx) = async_channel::unbounded();
    let metrics = Arc::new(Metrics::new());
    let client = CertStreamClient::new("ws://fake-url".to_string(), tx.clone(), 1.0, false, metrics);

    // 2. Act
    let completion_rx = run_client_with_signal(client, fake_ws).await;

    // 3. Assert
    // Wait for the client to signal completion. This is more robust than a timeout.
    completion_rx.await?;

    // Collect all messages from the channel.
    let mut received_domains = Vec::new();
    // Close the sender to signal the end of the stream
    drop(tx);
    while let Ok(domain) = rx.recv().await {
        received_domains.push(domain);
    }

    // Verify the received domains.
    assert_eq!(received_domains.len(), 2);
    assert!(received_domains.contains(&"test.example.com".to_string()));
    assert!(received_domains.contains(&"www.test.example.com".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_certstream_client_handles_invalid_messages() -> Result<()> {
    // 1. Arrange
    let invalid_message = r#"{"invalid": "structure"}"#;
    let fake_ws = FakeWebSocket::new_with_text_messages(vec![
        Some(Ok(invalid_message.to_string())),
        None, // Simulate connection close
    ]);
    let (tx, rx) = async_channel::unbounded();
    let metrics = Arc::new(Metrics::new());
    let client = CertStreamClient::new("ws://fake-url".to_string(), tx, 1.0, false, metrics);

    // 2. Act
    let completion_rx = run_client_with_signal(client, fake_ws).await;

    // 3. Assert
    // Wait for the client to signal completion.
    completion_rx.await?;

    // Check that no domains were sent to the channel.
    assert!(
        rx.try_recv().is_err(), // This will be an error because the channel is empty
        "Should not have received any domains for invalid message"
    );

    Ok(())
}
