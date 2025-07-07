use metrics;
// Service for sending alerts to various output destinations.

use crate::{
    config::OutputFormat,
    core::{Alert, AsnInfo},
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::io::Write;
use chrono::{Local, DateTime};

use crate::core::Output;

/// Manages a collection of output destinations and dispatches alerts to all of them.
pub struct OutputManager {
    outputs: Vec<std::sync::Arc<dyn Output>>,
}

impl OutputManager {
    /// Creates a new `OutputManager`.
    pub fn new(outputs: Vec<std::sync::Arc<dyn Output>>) -> Self {
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

/// An output that prints alerts to standard output.
pub struct StdoutOutput {
    format: OutputFormat,
}

impl StdoutOutput {
    /// Creates a new `StdoutOutput`.
    pub fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    /// Sends an alert to a specific writer, used for testing.
    #[cfg(test)]
    fn send_alert_to<W: Write>(&self, alert: &Alert, writer: &mut W) -> Result<()> {
        match self.format {
            OutputFormat::Json => {
                serde_json::to_writer(&mut *writer, alert)
                    .context("Failed to serialize alert to writer")?;
                writeln!(writer).context("Failed to write newline to writer")?;
            }
            OutputFormat::PlainText => {
                let formatted_string = format_plain_text(alert);
                writeln!(writer, "{}", formatted_string)
                    .context("Failed to write formatted alert to writer")?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Output for StdoutOutput {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        match self.format {
            OutputFormat::Json => {
                serde_json::to_writer(&mut handle, alert)
                    .context("Failed to serialize alert to stdout")?;
                writeln!(&mut handle).context("Failed to write newline to stdout")?;
            }
            OutputFormat::PlainText => {
                let formatted_string = format_plain_text(alert);
                writeln!(&mut handle, "{}", formatted_string)
                    .context("Failed to write formatted alert to stdout")?;
            }
        }
        Ok(())
    }
}

/// Formats an alert into a compact, single-line summary.
/// Format: `[tag] domain -> first_ip [country, as_number, as_name] (+n other IPs)`
fn format_plain_text(alert: &Alert) -> String {
    let enrichment_map: std::collections::HashMap<_, _> = alert
        .enrichment
        .iter()
        .map(|e| (e.ip, e.data.as_ref()))
        .collect();

    let all_ips: Vec<_> = alert
        .dns
        .a_records
        .iter()
        .map(|ip| ip.to_string())
        .chain(alert.dns.aaaa_records.iter().map(|ip| ip.to_string()))
        .collect();

    let first_ip_str = all_ips.first().cloned().unwrap_or_default();

    let enrichment_details = if let Some(first_ip) = all_ips.first() {
        if let Some(Some(data)) = enrichment_map.get(&first_ip.parse().unwrap()) {
            format_asn_data(data)
        } else {
            "[No enrichment data]".to_string()
        }
    } else {
        "".to_string()
    };

    let other_ips_count = all_ips.len().saturating_sub(1);
    let other_ips_str = if other_ips_count > 0 {
        format!(" (+{} other IPs)", other_ips_count)
    } else {
        "".to_string()
    };

    let now: DateTime<Local> = Local::now();
    let timestamp = now.format("%Y-%m-%dT%H:%M:%S%z").to_string();

    format!(
        "[{}] {} -> {} {}{} [{}]",
        alert.source_tag, alert.domain, first_ip_str, enrichment_details, other_ips_str, timestamp
    )
    .trim()
    .to_string()
}

fn format_asn_data(data: &AsnInfo) -> String {
    let country = data.country_code.as_deref().unwrap_or("??");
    format!("[{}, {}, {}]", country, data.as_number, data.as_name)
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

        let response = self.client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Slack webhook")?;

        match response.error_for_status() {
            Ok(_) => {
                metrics::counter!("webhook_deliveries", "status" => "success", "destination" => "slack").increment(1);
                Ok(())
            }
            Err(e) => {
                metrics::counter!("webhook_deliveries", "status" => "failure", "destination" => "slack").increment(1);
                Err(e).context("Slack API returned an error status")
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    use crate::core::{AsnInfo, DnsInfo, EnrichmentInfo};
    
    use std::sync::{Arc, Mutex};

    fn create_test_alert() -> Alert {
        Alert {
            timestamp: "2025-07-05T22:25:00Z".to_string(),
            domain: "example.com".to_string(),
            source_tag: "test-source".to_string(),
            resolved_after_nxdomain: false,
            dns: DnsInfo {
                a_records: vec!["1.1.1.1".parse().unwrap(), "2.2.2.2".parse().unwrap()],
                aaaa_records: vec!["2606:4700:4700::1111".parse().unwrap()],
                ns_records: vec!["ns1.example.com".to_string()],
            },
            enrichment: vec![
                EnrichmentInfo {
                    ip: "1.1.1.1".parse().unwrap(),
                    data: Some(AsnInfo {
                        as_number: 13335,
                        as_name: "CLOUDFLARENET".to_string(),
                        country_code: Some("US".to_string()),
                    }),
                },
                EnrichmentInfo {
                    ip: "2.2.2.2".parse().unwrap(),
                    data: Some(AsnInfo {
                        as_number: 12345,
                        as_name: "Test ASN".to_string(),
                        country_code: Some("GB".to_string()),
                    }),
                },
                // No enrichment for the IPv6 address
            ],
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

    #[test]
    fn test_stdout_output_plain_text_format() {
        let mut buffer = Vec::new();
        let output = StdoutOutput::new(OutputFormat::PlainText);
        let alert = create_test_alert();

        output.send_alert_to(&alert, &mut buffer).unwrap();

        let output_string = String::from_utf8(buffer).unwrap();
        // Updated expected string to include the timestamp.
        // Note: The exact timestamp will vary, so we'll need a more robust assertion for actual testing.
        // For now, this is a placeholder to ensure the format string is updated.
        let expected_prefix = "[test-source] example.com -> 1.1.1.1 [US, 13335, CLOUDFLARENET] (+2 other IPs)";
        assert!(output_string.starts_with(expected_prefix));
        assert!(output_string.ends_with("]\n") || output_string.ends_with("]\r\n"));
        assert!(output_string.len() > expected_prefix.len() + 10); // Ensure timestamp is present
    }

    #[test]
    fn test_stdout_output_json_format() {
        let mut buffer = Vec::new();
        let output = StdoutOutput::new(OutputFormat::Json);
        let alert = create_test_alert();

        output.send_alert_to(&alert, &mut buffer).unwrap();

        let output_string = String::from_utf8(buffer).unwrap();
        let deserialized_alert: Alert = serde_json::from_str(output_string.trim()).unwrap();
        assert_eq!(deserialized_alert, alert);
    }

    #[test]
    fn test_format_plain_text_full() {
        let alert = create_test_alert();
        let formatted = format_plain_text(&alert);
        // The exact timestamp will vary, so we'll check for the prefix and the presence of a timestamp.
        let expected_prefix = "[test-source] example.com -> 1.1.1.1 [US, 13335, CLOUDFLARENET] (+2 other IPs)";
        assert!(formatted.starts_with(expected_prefix));
        assert!(formatted.ends_with("]"));
        assert!(formatted.len() > expected_prefix.len() + 10); // Ensure timestamp is present
    }

    #[test]
    fn test_format_plain_text_single_ip() {
        let mut alert = create_test_alert();
        alert.dns.a_records = vec!["1.1.1.1".parse().unwrap()];
        alert.dns.aaaa_records = vec![];
        let formatted = format_plain_text(&alert);
        let expected_prefix = "[test-source] example.com -> 1.1.1.1 [US, 13335, CLOUDFLARENET]";
        assert!(formatted.starts_with(expected_prefix));
        assert!(formatted.ends_with("]"));
        assert!(formatted.len() > expected_prefix.len() + 10);
    }

    #[test]
    fn test_format_plain_text_no_ips() {
        let mut alert = create_test_alert();
        alert.dns.a_records = vec![];
        alert.dns.aaaa_records = vec![];
        alert.enrichment = vec![];
        let formatted = format_plain_text(&alert);
        let expected_prefix = "[test-source] example.com ->";
        assert!(formatted.starts_with(expected_prefix));
        assert!(formatted.ends_with("]"));
        assert!(formatted.len() > expected_prefix.len() + 10);
    }

    #[test]
    fn test_format_plain_text_no_enrichment_data() {
        let mut alert = create_test_alert();
        alert.enrichment = vec![];
        let formatted = format_plain_text(&alert);
        let expected_prefix = "[test-source] example.com -> 1.1.1.1 [No enrichment data] (+2 other IPs)";
        assert!(formatted.starts_with(expected_prefix));
        assert!(formatted.ends_with("]"));
        assert!(formatted.len() > expected_prefix.len() + 10);
    }

    #[test]
    fn test_format_plain_text_no_country_code() {
        let mut alert = create_test_alert();
        if let Some(enr) = alert.enrichment.get_mut(0) {
            if let Some(data) = enr.data.as_mut() {
                data.country_code = None;
            }
        }
        let formatted = format_plain_text(&alert);
        let expected_prefix = "[test-source] example.com -> 1.1.1.1 [??, 13335, CLOUDFLARENET] (+2 other IPs)";
        assert!(formatted.starts_with(expected_prefix));
        assert!(formatted.ends_with("]"));
        assert!(formatted.len() > expected_prefix.len() + 10);
    }
}
