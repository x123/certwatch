//! Integration tests for the CertStream WebSocket client
//! 
//! These tests verify the client's connection, reconnection, and message
//! processing logic using fake WebSocket implementations.

use anyhow::Result;
use async_trait::async_trait;
use certwatch::network::{WebSocketConnection, CertStreamClient};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

/// Fake WebSocket implementation for testing
#[derive(Debug)]
pub struct FakeWebSocket {
    messages: Vec<Option<Result<String, WsError>>>,
    current_index: Arc<Mutex<usize>>,
    connection_count: Arc<Mutex<usize>>,
}

impl FakeWebSocket {
    /// Creates a new fake WebSocket with predefined text messages
    pub fn new_with_text_messages(messages: Vec<Option<Result<String, WsError>>>) -> Self {
        Self {
            messages,
            current_index: Arc::new(Mutex::new(0)),
            connection_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Returns the number of times this fake was "connected" to
    pub fn connection_count(&self) -> usize {
        *self.connection_count.lock().unwrap()
    }

    /// Simulates a new connection by incrementing the connection counter
    pub fn connect(&self) {
        let mut count = self.connection_count.lock().unwrap();
        *count += 1;
    }
}

#[async_trait]
impl WebSocketConnection for FakeWebSocket {
    async fn read_message(&mut self) -> Option<Result<Message, WsError>> {
        let mut index = self.current_index.lock().unwrap();
        if *index < self.messages.len() {
            let message_result = &self.messages[*index];
            *index += 1;
            
            match message_result {
                Some(Ok(text)) => Some(Ok(Message::Text(text.clone()))),
                Some(Err(_e)) => {
                    // Create a simple WebSocket error for testing
                    Some(Err(WsError::ConnectionClosed))
                }
                None => None,
            }
        } else {
            None
        }
    }
}

#[tokio::test]
async fn test_certstream_client_basic_functionality() {
    // Create a sample valid certstream message
    let sample_message = r#"{
        "message_type": "certificate_update",
        "data": {
            "update_type": "X509LogEntry",
            "leaf_cert": {
                "all_domains": ["test.example.com", "www.test.example.com"]
            }
        }
    }"#;

    // Set up fake WebSocket with one valid message, then connection close
    let fake_ws = FakeWebSocket::new_with_text_messages(vec![
        Some(Ok(sample_message.to_string())),
        None, // Simulate connection close
    ]);

    // Create a channel to receive parsed domains
    let (tx, mut rx) = mpsc::channel::<Vec<String>>(10);

    // Create the client
    let client = CertStreamClient::new("ws://fake-url".to_string(), tx);

    // Run the client with the fake WebSocket (this should process one message then stop)
    let client_handle = tokio::spawn(async move {
        client.run_with_connection(Box::new(fake_ws)).await
    });

    // Wait for the parsed domains
    let received_domains = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx.recv()
    ).await;

    // Verify we received the expected domains
    assert!(received_domains.is_ok(), "Should have received domains within timeout");
    let domains = received_domains.unwrap().unwrap();
    assert_eq!(domains.len(), 2);
    assert!(domains.contains(&"test.example.com".to_string()));
    assert!(domains.contains(&"www.test.example.com".to_string()));

    // Clean up the client task
    client_handle.abort();
}

#[tokio::test]
async fn test_certstream_client_handles_invalid_messages() {
    // Create an invalid JSON message
    let invalid_message = r#"{"invalid": "structure"}"#;

    // Set up fake WebSocket with invalid message, then connection close
    let fake_ws = FakeWebSocket::new_with_text_messages(vec![
        Some(Ok(invalid_message.to_string())),
        None, // Simulate connection close
    ]);

    // Create a channel to receive parsed domains
    let (tx, mut rx) = mpsc::channel::<Vec<String>>(10);

    // Create the client
    let client = CertStreamClient::new("ws://fake-url".to_string(), tx);

    // Run the client with the fake WebSocket
    let client_handle = tokio::spawn(async move {
        client.run_with_connection(Box::new(fake_ws)).await
    });

    // Wait a short time to see if any domains are received (there shouldn't be any)
    let received_domains = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx.recv()
    ).await;

    // The client should terminate normally after processing the invalid message and connection close
    let client_result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        client_handle
    ).await;

    // Debug: print what we actually received
    match &received_domains {
        Ok(Some(domains)) => println!("Received domains: {:?}", domains),
        Ok(None) => println!("Received None (channel closed)"),
        Err(_) => println!("Timeout - no domains received"),
    }

    // Verify no domains were received (invalid message should be logged and ignored)
    // We expect either a timeout (no message sent) or channel closed (None)
    match received_domains {
        Ok(Some(_)) => panic!("Should not have received any domains for invalid message"),
        Ok(None) => {
            // Channel closed without sending domains - this is expected
            println!("Test passed: Channel closed without sending domains");
        }
        Err(_) => {
            // Timeout - also acceptable as it means no domains were sent
            println!("Test passed: No domains received within timeout");
        }
    }
    
    // Verify the client terminated normally
    assert!(client_result.is_ok(), "Client should have terminated normally");
}
