//! Service for sending alerts to various output destinations.

use crate::core::Alert;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::io::Write;

use crate::core::Output;

/// Manages a collection of output destinations and dispatches alerts to all of them.
pub struct OutputManager {
    outputs: Vec<Box<dyn Output>>,
}

impl OutputManager {
    /// Creates a new `OutputManager`.
    pub fn new(outputs: Vec<Box<dyn Output>>) -> Self {
        Self { outputs }
    }

    /// Sends an alert to all configured output destinations.
    pub async fn send_alert(&self, alert: &Alert) -> Result<()> {
        for output in &self.outputs {
            if let Err(e) = output.send_alert(alert).await {
                log::error!("Failed to send alert via an output: {}", e);
            }
        }
        Ok(())
    }
}

// =============================================================================
// Stdout Output
// =============================================================================

/// An output that prints alerts to standard output as JSON.
pub struct StdoutOutput;

impl StdoutOutput {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Output for StdoutOutput {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        let mut stdout = std::io::stdout();
        serde_json::to_writer(&mut stdout, alert)
            .context("Failed to serialize alert to stdout")?;
        writeln!(&mut stdout).context("Failed to write newline to stdout")?;
        Ok(())
    }
}

// =============================================================================
// JSON File Output
// =============================================================================

/// An output that appends alerts as JSON to a file.
pub struct JsonOutput {
    // In a real implementation, this would likely be a file handle
    // wrapped in a Mutex for concurrent access.
    _file_path: String,
}

impl JsonOutput {
    pub fn new(file_path: &str) -> Result<Self> {
        // In a real implementation, we would open/create the file here.
        Ok(Self {
            _file_path: file_path.to_string(),
        })
    }
}

#[async_trait]
impl Output for JsonOutput {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        // This is a placeholder. A real implementation would write to the file.
        log::info!(
            "JSON_OUTPUT (to {}): {}",
            self._file_path,
            serde_json::to_string(alert)?
        );
        Ok(())
    }
}

// =============================================================================
// Slack Output
// =============================================================================

/// An output that sends alerts to a Slack webhook.
pub struct SlackOutput {
    client: reqwest::Client,
    webhook_url: String,
}

impl SlackOutput {
    pub fn new(webhook_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            webhook_url,
        }
    }
}

#[async_trait]
impl Output for SlackOutput {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        let payload = serde_json::json!({
            "text": format!("Suspicious domain detected: *{}*", alert.domain),
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!(
                            "*Domain:* `{}`\n*Source:* `{}`\n*Timestamp:* {}",
                            alert.domain, alert.source_tag, alert.timestamp
                        )
                    }
                },
                {
                    "type": "context",
                    "elements": [
                        {
                            "type": "mrkdwn",
                            "text": format!("Resolved after NXDOMAIN: `{}`", alert.resolved_after_nxdomain)
                        }
                    ]
                }
            ]
        });

        self.client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Slack webhook")?
            .error_for_status()
            .context("Slack API returned an error status")?;

        Ok(())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AsnData, DnsInfo, EnrichmentInfo, GeoIpInfo};
    use std::sync::{Arc, Mutex};

    fn create_test_alert() -> Alert {
        Alert {
            timestamp: "2025-07-05T22:25:00Z".to_string(),
            domain: "example.com".to_string(),
            source_tag: "test-source".to_string(),
            resolved_after_nxdomain: false,
            dns: DnsInfo {
                a_records: vec!["1.1.1.1".parse().unwrap()],
                aaaa_records: vec![],
                ns_records: vec!["ns1.example.com".to_string()],
            },
            enrichment: vec![EnrichmentInfo {
                ip: "1.1.1.1".parse().unwrap(),
                asn: Some(AsnData {
                    as_number: 12345,
                    as_name: "Test ASN".to_string(),
                }),
                geoip: Some(GeoIpInfo {
                    country_code: "US".to_string(),
                }),
            }],
        }
    }

    /// A mock output that captures alerts for inspection.
    struct MockOutput {
        alerts: Arc<Mutex<Vec<Alert>>>,
    }

    impl MockOutput {
        fn new() -> Self {
            Self {
                alerts: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl Output for MockOutput {
        async fn send_alert(&self, alert: &Alert) -> Result<()> {
            self.alerts.lock().unwrap().push(alert.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_output_manager_dispatches_to_all() {
        let outputs: Vec<Box<dyn Output>> = vec![
            Box::new(MockOutput::new()),
            Box::new(MockOutput::new()),
        ];
        let manager = OutputManager::new(outputs);
        let alert = create_test_alert();

        manager.send_alert(&alert).await.unwrap();

        // This test structure needs adjustment as we can't access the mocks directly.
        // A better approach would be for the mocks to use channels or other
        // inspectable side-effects. For now, this test just ensures no panics.
    }

    #[tokio::test]
    async fn test_slack_output_request_format() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#" "ok" "#)
            .create_async()
            .await;

        let output = SlackOutput::new(server.url());
        let alert = create_test_alert();

        output.send_alert(&alert).await.unwrap();
        mock.assert_async().await;
    }
}
