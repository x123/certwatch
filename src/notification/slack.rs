//! A client for sending notifications to Slack.

use crate::core::AggregatedAlert;
use crate::formatting::TextFormatter;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::task;
use tracing::{error, info, instrument};

/// A trait for clients that can send batches of alerts.
#[async_trait]
pub trait SlackClientTrait: Send + Sync {
    /// Sends a batch of alerts.
    async fn send_batch(&self, alerts: &[AggregatedAlert]) -> anyhow::Result<()>;
}

/// A client for sending messages to a Slack webhook.
pub struct SlackClient {
    webhook_url: String,
    formatter: Box<dyn TextFormatter>,
    timeout: std::time::Duration,
}

impl SlackClient {
    /// Creates a new `SlackClient`.
    pub fn new(webhook_url: String, formatter: Box<dyn TextFormatter>) -> Self {
        Self {
            webhook_url,
            formatter,
            timeout: std::time::Duration::from_secs(10),
        }
    }

    /// Sends the request in a blocking manner.
    fn send_request(
        client: reqwest::blocking::Client,
        webhook_url: &str,
        payload: &Value,
    ) -> anyhow::Result<()> {
        let response = client.post(webhook_url).json(payload).send();

        match response {
            Ok(res) => {
                if res.status().is_success() {
                    info!("Successfully sent batch to Slack.");
                    Ok(())
                } else {
                    let status = res.status();
                    let text = res.text().unwrap_or_default();
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

#[async_trait]
impl SlackClientTrait for SlackClient {
    /// Formats and sends a batch of alerts to the configured Slack webhook.
    #[instrument(skip(self, alerts), fields(count = alerts.len()))]
    async fn send_batch(&self, alerts: &[AggregatedAlert]) -> anyhow::Result<()> {
        if alerts.is_empty() {
            return Ok(());
        }

        info!("Formatting and sending batch to Slack.");
        let formatted_text = self.formatter.format_batch(alerts);
        let payload = json!({ "text": formatted_text });

        let webhook_url = self.webhook_url.clone();
        let timeout = self.timeout;
        let result = task::spawn_blocking(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(timeout)
                .build()
                .unwrap();
            Self::send_request(client, &webhook_url, &payload)
        })
        .await;

        match result {
            Ok(Ok(())) => {
                info!(
                    "Successfully sent batch of {} alerts to Slack.",
                    alerts.len()
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(e) => {
                error!(error = %e, "Slack notification task failed");
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

    fn create_test_alert(domain: &str) -> AggregatedAlert {
        AggregatedAlert {
            alert: Alert {
                domain: domain.to_string(),
                ..Default::default()
            },
            deduplicated_count: 0,
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

    #[test]
    fn test_slack_client_handles_timeout() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Arrange
            let server = MockServer::start().await;
            let formatter = Box::new(SlackTextFormatter);
            let alerts = vec![create_test_alert("test.com")];

            Mock::given(method("POST"))
                .and(path("/webhook"))
                .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)))
                .mount(&server)
                .await;

            let mut client = SlackClient::new(format!("{}/webhook", server.uri()), formatter);
            client.timeout = std::time::Duration::from_millis(500);

            // Act
            let result = client.send_batch(&alerts).await;

            // Assert
            assert!(result.is_err());
            let err = result.unwrap_err();
            let is_timeout = err
                .chain()
                .any(|cause| cause.downcast_ref::<reqwest::Error>().map_or(false, |e| e.is_timeout()));

            assert!(is_timeout, "Error should be a timeout error, but was: {}", err);
        });
    }
}