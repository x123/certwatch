//! Network client for CertStream WebSocket connection
//! 
//! This module handles connecting to the certstream websocket, parsing
//! messages, and managing reconnection logic.

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

/// Parses a raw certstream JSON message and extracts domain names
/// 
/// # Arguments
/// * `text` - The raw JSON string from certstream
/// 
/// # Returns
/// * `Ok(Vec<String>)` containing all domain names found in the message
/// * `Err` if the JSON is malformed or doesn't match expected structure
pub fn parse_message(text: &str) -> Result<Vec<String>> {
    // Struct for parsing the "domains-only" certstream JSON format
    #[derive(Deserialize)]
    struct CertStreamMessage {
        data: Vec<String>,
    }

    let message: CertStreamMessage = serde_json::from_str(text)?;
    Ok(message.data)
}

/// Trait for WebSocket connections to enable testing with fake implementations
#[async_trait]
pub trait WebSocketConnection: Send + Sync {
    /// Reads the next message from the WebSocket connection
    /// 
    /// # Returns
    /// * `Some(Ok(Message))` if a message was successfully received
    /// * `Some(Err(error))` if there was an error reading the message
    /// * `None` if the connection has been closed
    async fn read_message(&mut self) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>>;
}

/// CertStream WebSocket client that connects to the certstream service
/// and processes incoming certificate transparency log entries
pub struct CertStreamClient {
    url: String,
    output_tx: tokio::sync::mpsc::Sender<Vec<String>>,
}

impl CertStreamClient {
    /// Creates a new CertStream client
    /// 
    /// # Arguments
    /// * `url` - The WebSocket URL to connect to (e.g., "wss://certstream.calidog.io")
    /// * `output_tx` - Channel sender to send parsed domain lists to the next stage
    pub fn new(url: String, output_tx: tokio::sync::mpsc::Sender<Vec<String>>) -> Self {
        Self { url, output_tx }
    }

    /// Runs the client with a custom WebSocket connection (primarily for testing)
    /// 
    /// This method processes messages from the provided connection until it closes,
    /// then returns. It does not implement reconnection logic.
    pub async fn run_with_connection(&self, mut connection: Box<dyn WebSocketConnection>) -> Result<()> {
        log::info!("Starting CertStream client message processing");

        loop {
            match connection.read_message().await {
                Some(Ok(Message::Text(text))) => {
                    // Parse the message and extract domains
                    match parse_message(&text) {
                        Ok(domains) => {
                            if !domains.is_empty() {
                                log::debug!("Parsed {} domains from certstream message", domains.len());
                                
                                // Send domains to the next stage
                                if let Err(e) = self.output_tx.send(domains).await {
                                    log::error!("Failed to send domains to output channel: {}", e);
                                    return Err(anyhow::anyhow!("Output channel closed: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to parse certstream message: {}", e);
                            // Continue processing other messages
                        }
                    }
                }
                Some(Ok(Message::Binary(_))) => {
                    log::debug!("Received binary message, ignoring");
                }
                Some(Ok(Message::Ping(_))) => {
                    log::debug!("Received ping message");
                }
                Some(Ok(Message::Pong(_))) => {
                    log::debug!("Received pong message");
                }
                Some(Ok(Message::Close(_))) => {
                    log::info!("Received close message from server");
                    break;
                }
                Some(Ok(Message::Frame(_))) => {
                    log::debug!("Received frame message, ignoring");
                }
                Some(Err(e)) => {
                    log::error!("WebSocket error: {}", e);
                    return Err(anyhow::anyhow!("WebSocket error: {}", e));
                }
                None => {
                    log::info!("WebSocket connection closed");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Runs the client with automatic reconnection to the configured URL
    /// 
    /// This method implements the main client loop with exponential backoff
    /// reconnection logic. It will continue running until explicitly stopped.
    pub async fn run(&self) -> Result<()> {
        let mut backoff_ms = 1000; // Start with 1 second
        const MAX_BACKOFF_MS: u64 = 60000; // Max 60 seconds

        loop {
            log::info!("Attempting to connect to {}", self.url);

            match self.connect_and_run().await {
                Ok(()) => {
                    log::info!("Connection closed normally");
                    backoff_ms = 1000; // Reset backoff on successful connection
                }
                Err(e) => {
                    log::error!("Connection failed: {}", e);
                }
            }

            log::info!("Reconnecting in {} ms", backoff_ms);
            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;

            // Exponential backoff with jitter
            backoff_ms = std::cmp::min(backoff_ms * 2, MAX_BACKOFF_MS);
        }
    }

    /// Connects to the WebSocket URL and runs the message processing loop
    async fn connect_and_run(&self) -> Result<()> {
        use tokio_tungstenite::{connect_async, Connector};

        // For local testing with self-signed certificates, use a custom connector
        if self.url.starts_with("wss://") && self.url.contains("127.0.0.1") {
            // Create TLS connector that accepts self-signed certificates
            let mut tls_connector = native_tls::TlsConnector::builder();
            tls_connector.danger_accept_invalid_certs(true);
            tls_connector.danger_accept_invalid_hostnames(true);
            let tls_connector = tls_connector.build()
                .map_err(|e| anyhow::anyhow!("Failed to create TLS connector: {}", e))?;
            
            let connector = Connector::NativeTls(tls_connector);

            // Connect with custom TLS configuration
            let request = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(&self.url).unwrap();
            let (ws_stream, _) = tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector)).await
                .map_err(|e| anyhow::anyhow!("Failed to connect to {} with custom TLS: {}", self.url, e))?;

            log::info!("Connected to {} with custom TLS", self.url);
            return self.process_messages(ws_stream).await;
        }

        // Standard connection for non-local or non-SSL URLs
        let (ws_stream, _) = connect_async(&self.url).await
            .map_err(|e| anyhow::anyhow!("Failed to connect to {}: {}", self.url, e))?;

        log::info!("Connected to {}", self.url);
        self.process_messages(ws_stream).await
    }

    /// Process WebSocket messages from any type of stream
    async fn process_messages<S>(&self, mut ws_stream: S) -> Result<()>
    where
        S: futures_util::stream::Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
            + futures_util::sink::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
            + Unpin,
    {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        // Send a ping to the server to keep the connection alive
        ws_stream.send(Message::Ping(vec![])).await?;

        let mut read = ws_stream;

        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    // Parse the message and extract domains
                    match parse_message(&text) {
                        Ok(domains) => {
                            if !domains.is_empty() {
                                log::debug!("Parsed {} domains from certstream message", domains.len());
                                
                                // Send domains to the next stage
                                if let Err(e) = self.output_tx.send(domains).await {
                                    log::error!("Failed to send domains to output channel: {}", e);
                                    return Err(anyhow::anyhow!("Output channel closed: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to parse certstream message: {}", e);
                            // Continue processing other messages
                        }
                    }
                }
                Ok(Message::Binary(_)) => {
                    log::debug!("Received binary message, ignoring");
                }
                Ok(Message::Ping(_)) => {
                    log::debug!("Received ping message");
                }
                Ok(Message::Pong(_)) => {
                    log::debug!("Received pong message");
                }
                Ok(Message::Close(_)) => {
                    log::info!("Received close message from server");
                    break;
                }
                Ok(Message::Frame(_)) => {
                    log::debug!("Received frame message, ignoring");
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("WebSocket error: {}", e));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message_success() {
        let sample_json = r#"{"data": ["example.com", "www.example.com", "*.example.com"]}"#;

        let result = parse_message(sample_json);
        assert!(result.is_ok(), "Expected successful parsing, got error: {:?}", result.err());
        
        let domains = result.unwrap();
        assert_eq!(domains.len(), 3);
        assert!(domains.contains(&"example.com".to_string()));
        assert!(domains.contains(&"www.example.com".to_string()));
        assert!(domains.contains(&"*.example.com".to_string()));
    }

    #[test]
    fn test_parse_message_invalid_json() {
        let invalid_json = r#"{"invalid": "json structure"#;
        
        let result = parse_message(invalid_json);
        assert!(result.is_err(), "Expected error for invalid JSON");
    }

    #[test]
    fn test_parse_message_missing_fields() {
        let incomplete_json = r#"{"message_type": "certificate_update"}"#;
        
        let result = parse_message(incomplete_json);
        assert!(result.is_err(), "Expected error for missing required 'data' field");
    }

    #[test]
    fn test_parse_message_empty_domains() {
        let empty_domains_json = r#"{"data": []}"#;
        
        let result = parse_message(empty_domains_json);
        assert!(result.is_ok(), "Expected successful parsing even with empty domains");
        
        let domains = result.unwrap();
        assert_eq!(domains.len(), 0);
    }
}
