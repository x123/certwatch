//! A client for sending notifications to Slack.

use crate::formatting::TextFormatter;
use async_trait::async_trait;
use serde_json::json;
use tracing::{error, info, instrument};

/// A trait for clients that can send batches of alerts.
#[async_trait]
pub trait SlackClientTrait: Send + Sync {
    /// Sends a batch of alerts.
    async fn send_batch(&self, alerts: &[crate::core::Alert]) -> anyhow::Result<()>;
}

/// A client for sending messages to a Slack webhook.
pub struct SlackClient {
    client: reqwest::Client,
    webhook_url: String,
    formatter: Box<dyn TextFormatter>,
}

impl SlackClient {
    /// Creates a new `SlackClient`.
    ///
    /// # Arguments
    /// * `webhook_url` - The Slack incoming webhook URL.
    /// * `formatter` - A formatter to convert alerts into a string payload.
    pub fn new(webhook_url: String, formatter: Box<dyn TextFormatter>) -> Self {
        Self {
            client: reqwest::Client::new(),
            webhook_url,
            formatter,
        }
    }
}

#[async_trait]
impl SlackClientTrait for SlackClient {
    /// Formats and sends a batch of alerts to the configured Slack webhook.
    ///
    /// # Arguments
    /// * `alerts` - A slice of `Alert`s to send.
    #[instrument(skip(self, alerts), fields(count = alerts.len()))]
    async fn send_batch(&self, alerts: &[crate::core::Alert]) -> anyhow::Result<()> {
        if alerts.is_empty() {
            return Ok(());
        }

        info!("Formatting and sending batch to Slack.");

        let formatted_text = self.formatter.format_batch(alerts);
        let payload = json!({ "text": formatted_text });

        let response = self
            .client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await;

        match response {
            Ok(res) => {
                if res.status().is_success() {
                    info!("Successfully sent batch of {} alerts to Slack.", alerts.len());
                    Ok(())
                } else {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    error!(
                        status = %status,
                        body = %text,
                        "Failed to send Slack notification"
                    );
                    anyhow::bail!(
                        "Failed to send Slack notification: status {}, body: {}",
                        status,
                        text
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "HTTP request to Slack failed");
                Err(e.into())
            }
        }
    }
}

#[cfg(test)]
mod slack_client_tests {
    use super::*;
    use crate::core::Alert;
    use crate::formatting::SlackTextFormatter;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_alert(domain: &str) -> Alert {
        Alert {
            domain: domain.to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_slack_client_send_batch_success() {
        // Arrange
        let server = MockServer::start().await;
        let formatter = Box::new(SlackTextFormatter);
        let alerts = vec![create_test_alert("test.com")];
        let expected_body = json!({ "text": formatter.format_batch(&alerts) });

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .and(body_json(&expected_body))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = SlackClient::new(format!("{}/webhook", server.uri()), formatter);

        // Act
        let result = client.send_batch(&alerts).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_slack_client_handles_server_error() {
        // Arrange
        let server = MockServer::start().await;
        let formatter = Box::new(SlackTextFormatter);
        let alerts = vec![create_test_alert("test.com")];

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = SlackClient::new(format!("{}/webhook", server.uri()), formatter);

        // Act
        let result = client.send_batch(&alerts).await;

        // Assert
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_slack_client_handles_timeout() {
        // Arrange
        let server = MockServer::start().await;
        let formatter = Box::new(SlackTextFormatter);
        let alerts = vec![create_test_alert("test.com")];

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)))
            .mount(&server)
            .await;

        let client = SlackClient {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(500))
                .build()
                .unwrap(),
            webhook_url: format!("{}/webhook", server.uri()),
            formatter,
        };

        // Act
        let result = client.send_batch(&alerts).await;

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.downcast_ref::<reqwest::Error>().unwrap().is_timeout());
    }
}