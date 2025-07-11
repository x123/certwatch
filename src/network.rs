// Network client for CertStream WebSocket connection
//
// This module handles connecting to the certstream websocket, parsing
// messages, and managing reconnection logic.

use crate::{internal_metrics::Metrics, utils::heartbeat::run_heartbeat};
use anyhow::Result;
use async_trait::async_trait;
use rand::Rng;
use serde::Deserialize;
use std::sync::Arc;
use async_channel::Sender;
use tokio::sync::watch;
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
    output_tx: Sender<String>,
    sample_rate: f64,
    allow_invalid_certs: bool,
    metrics: Arc<Metrics>,
}

impl CertStreamClient {
    /// Creates a new CertStream client
    ///
    /// # Arguments
    /// * `url` - The WebSocket URL to connect to (e.g., "wss://certstream.calidog.io")
    /// * `output_tx` - Channel sender to send individual domains to the next stage
    /// * `sample_rate` - A float between 0.0 and 1.0 indicating the percentage of domains to process
    /// * `allow_invalid_certs` - Whether to allow self-signed TLS certificates
    pub fn new(
        url: String,
        output_tx: Sender<String>,
        sample_rate: f64,
        allow_invalid_certs: bool,
        metrics: Arc<Metrics>,
    ) -> Self {
        assert!(
            (0.0..=1.0).contains(&sample_rate),
            "sample_rate must be between 0.0 and 1.0"
        );
        Self {
            url,
            output_tx,
            sample_rate,
            allow_invalid_certs,
            metrics,
        }
    }

    /// Runs the client with a custom WebSocket connection (primarily for testing)
    ///
    /// This method processes messages from the provided connection until it closes,
    /// then returns. It does not implement reconnection logic.
    pub async fn run_with_connection(
        &self,
        mut connection: Box<dyn WebSocketConnection>,
    ) -> Result<()> {
        tracing::info!("Starting CertStream client message processing");
        while let Some(msg_result) = connection.read_message().await {
            match msg_result {
                Ok(message) => {
                    if let Err(e) = self.handle_message(message).await {
                        // A closed channel is a critical error that should stop the client
                        return Err(e);
                    }
                }
                Err(e) => {
                    tracing::error!("WebSocket error: {}", e);
                    return Err(anyhow::anyhow!("WebSocket error: {}", e));
                }
            }
        }
        tracing::info!("WebSocket connection closed");
        Ok(())
    }

    /// Runs the client with automatic reconnection to the configured URL
    /// 
    /// This method implements the main client loop with exponential backoff
    /// reconnection logic. It will continue running until explicitly stopped.
    pub async fn run(&self, mut shutdown_rx: watch::Receiver<()>) -> Result<()> {
        let mut backoff_ms = 1000; // Start with 1 second
        const MAX_BACKOFF_MS: u64 = 60000; // Max 60 seconds

        // Spawn the heartbeat task
        let hb_shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            run_heartbeat("CertStreamClient", hb_shutdown_rx).await;
        });

        loop {
            tokio::select! {
                biased;

                _ = shutdown_rx.changed() => {
                    tracing::info!("CertStream client received shutdown signal.");
                    return Ok(());
                }

                conn_result = self.connect_and_run() => {
                     match conn_result {
                        Ok(()) => {
                            tracing::info!("Connection closed normally");
                            backoff_ms = 1000; // Reset backoff on successful connection
                        }
                        Err(e) => {
                            tracing::error!("Connection failed: {}", e);
                            self.metrics.increment_websocket_disconnects();
                        }
                    }
                }
            }

            // Before retrying, mark the connection as down.
            self.metrics.set_websocket_connection_status(0);

            // Wait for the backoff period, but also listen for shutdown.
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)) => {
                    // Continue to the next iteration to retry connection.
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!("Shutdown signal received during backoff. CertStream client shutting down.");
                    return Ok(());
                }
            }

            tracing::info!("Reconnecting...");
            // Exponential backoff with jitter
            backoff_ms = std::cmp::min(backoff_ms * 2, MAX_BACKOFF_MS);
        }
    }

    /// Connects to the WebSocket URL and runs the message processing loop
    async fn connect_and_run(&self) -> Result<()> {
        use tokio_tungstenite::Connector;

        let connector = if self.allow_invalid_certs {
            let mut tls_connector = native_tls::TlsConnector::builder();
            tls_connector.danger_accept_invalid_certs(true);
            let tls_connector = tls_connector
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create TLS connector: {}", e))?;
            Some(Connector::NativeTls(tls_connector))
        } else {
            None
        };

        let (ws_stream, _) =
            tokio_tungstenite::connect_async_tls_with_config(&self.url, None, false, connector)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to connect to {}: {}", self.url, e))?;

        tracing::info!("Connected to {}", self.url);
        self.metrics.set_websocket_connection_status(1);
        self.process_messages(ws_stream).await
    }

    /// Process WebSocket messages from any type of stream
    async fn process_messages<S>(&self, mut ws_stream: S) -> Result<()>
    where
        S: futures_util::stream::Stream<
                Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
            > + futures_util::sink::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
            + Unpin,
    {
        use futures_util::{SinkExt, StreamExt};

        // Send a ping to the server to keep the connection alive
        ws_stream.send(Message::Ping(vec![].into())).await?;

        while let Some(msg_result) = ws_stream.next().await {
            match msg_result {
                Ok(message) => {
                    if let Err(e) = self.handle_message(message).await {
                        // A closed channel is a critical error that should stop the client
                        return Err(e);
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("WebSocket error: {}", e));
                }
            }
        }

        Ok(())
    }

    /// Handles a single WebSocket message, including parsing, sampling, and sending
    async fn handle_message(&self, message: Message) -> Result<()> {
        match message {
            Message::Text(text) => {
                match parse_message(&text) {
                    Ok(domains) => {
                        if domains.is_empty() {
                            return Ok(());
                        }

                        // Before sampling, record the total number of domains ingested.
                        self.metrics.domains_ingested_total.increment(domains.len() as u64);

                        let sampled_domains = self.sample_domains(domains);

                        if !sampled_domains.is_empty() {
                            for domain in sampled_domains {
                                if self.output_tx.send(domain).await.is_err() {
                                    // This error means the receiver has been dropped, which implies
                                    // the main application is shutting down. We should stop.
                                    tracing::info!("Domain channel closed. CertStream client shutting down.");
                                    return Err(anyhow::anyhow!("Domain channel closed"));
                                }
                                metrics::gauge!("domains_queued").increment(1.0);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse certstream message: {}", e);
                    }
                }
            }
            Message::Close(_) => {
                tracing::info!("Received close message from server");
                // This will be handled by the calling loop, which will exit
            }
            _ => {
                // Ignore other message types (Ping, Pong, Binary, etc.)
            }
        }
        Ok(())
    }

    /// Applies sampling to a vector of domains
    fn sample_domains(&self, domains: Vec<String>) -> Vec<String> {
        if self.sample_rate >= 1.0 {
            return domains;
        }

        let mut rng = rand::rng();
        domains
            .into_iter()
            .filter(|_| rng.random::<f64>() < self.sample_rate)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_metrics::Metrics;
    use std::sync::Arc;
    use async_channel;

    fn create_test_metrics() -> Arc<Metrics> {
        // The metrics system uses a global recorder, so we can't easily
        // isolate it for tests. For unit tests, we just need a valid
        // `Metrics` instance that doesn't panic.
        Arc::new(Metrics::new_for_test())
    }

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

    #[test]
    fn test_sample_domains() {
        let (tx, _) = async_channel::unbounded();
        let metrics = create_test_metrics();
        let domains: Vec<String> = (0..10000).map(|i| format!("domain{}.com", i)).collect();

        // Test case 1: sample_rate = 0.5 (50%)
        let client_half =
            CertStreamClient::new("".to_string(), tx.clone(), 0.5, false, metrics.clone());
        let sampled_half = client_half.sample_domains(domains.clone());
        // Check if the sampled count is roughly 50% +/- 5%
        let count_half = sampled_half.len();
        assert!(
            (4500..=5500).contains(&count_half),
            "Expected around 5000 domains, but got {}",
            count_half
        );

        // Test case 2: sample_rate = 1.0 (100%)
        let client_full =
            CertStreamClient::new("".to_string(), tx.clone(), 1.0, false, metrics.clone());
        let sampled_full = client_full.sample_domains(domains.clone());
        assert_eq!(
            sampled_full.len(),
            domains.len(),
            "Expected all domains with sample_rate = 1.0"
        );

        // Test case 3: sample_rate = 0.0 (0%)
        let client_none = CertStreamClient::new("".to_string(), tx, 0.0, false, metrics);
        let sampled_none = client_none.sample_domains(domains);
        assert_eq!(
            sampled_none.len(),
            0,
            "Expected no domains with sample_rate = 0.0"
        );
    }
}
