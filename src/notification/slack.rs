//! Defines the data structures for creating a rich Slack message payload.
//!
//! The structs defined here are used to construct a JSON object that conforms
//! to the Slack API's `chat.postMessage` format, specifically using the
//! "attachments" field to create a well-formatted, readable alert summary.

use serde::Serialize;

/// Represents the top-level structure of a Slack message payload.
#[derive(Serialize, Debug)]
pub struct SlackMessage {
    /// The main text of the message (optional, can be in an attachment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A collection of attachments, where the rich formatting is defined.
    pub attachments: Vec<Attachment>,
}

/// Represents a single attachment in a Slack message.
#[derive(Serialize, Debug)]
pub struct Attachment {
    /// A color bar to display on the left side of the attachment.
    pub color: String,
    /// A collection of fields to display in a two-column layout.
    pub fields: Vec<Field>,
}

/// Represents a single key-value field within an attachment.
#[derive(Serialize, Debug)]
pub struct Field {
    /// The title of the field.
    pub title: String,
    /// The value of the field.
    pub value: String,
    /// Whether the field is short enough to be displayed next to other short fields.
    pub short: bool,
}

/// Builds a Slack message payload from a slice of alerts.
///
/// # Arguments
/// * `alerts` - A slice of `Alert`s to include in the message.
///
/// # Returns
/// A `SlackMessage` ready to be serialized and sent to Slack.
pub fn build_slack_message(alerts: &[crate::core::Alert]) -> SlackMessage {
    let attachments = alerts
        .iter()
        .map(|alert| {
            let mut fields = vec![
                Field {
                    title: "Domain".to_string(),
                    value: alert.domain.clone(),
                    short: true,
                },
                Field {
                    title: "Source".to_string(),
                    value: alert.source_tag.clone(),
                    short: true,
                },
            ];

            if alert.resolved_after_nxdomain {
                fields.push(Field {
                    title: "Status".to_string(),
                    value: "Newly Resolved".to_string(),
                    short: true,
                });
            }

            if let Some(info) = alert.enrichment.first().and_then(|e| e.data.as_ref()) {
                fields.push(Field {
                    title: "ASN".to_string(),
                    value: info.as_name.clone(),
                    short: false,
                });
            }

            Attachment {
                color: "#FF0000".to_string(), // Red for alerts
                fields,
            }
        })
        .collect();

    SlackMessage {
        text: Some(format!(
            "CertWatch detected {} new suspicious domains:",
            alerts.len()
        )),
        attachments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Alert, AsnInfo, DnsInfo, EnrichmentInfo};
    use std::net::Ipv4Addr;

    fn create_test_alert(domain: &str, source: &str, resolved: bool, asn_name: Option<&str>) -> Alert {
        let enrichment = if let Some(name) = asn_name {
            vec![EnrichmentInfo {
                ip: Ipv4Addr::new(1, 1, 1, 1).into(),
                data: Some(AsnInfo {
                    as_number: 13335,
                    as_name: name.to_string(),
                    country_code: Some("US".to_string()),
                }),
            }]
        } else {
            vec![]
        };

        Alert {
            timestamp: "2024-01-01T12:00:00Z".to_string(),
            domain: domain.to_string(),
            source_tag: source.to_string(),
            resolved_after_nxdomain: resolved,
            dns: DnsInfo::default(),
            enrichment,
        }
    }

    #[test]
    fn test_build_slack_message_single_alert() {
        let alert = create_test_alert("evil.com", "phishing", false, Some("CLOUDFLARE, INC."));
        let message = build_slack_message(&[alert]);

        assert_eq!(message.text.unwrap(), "CertWatch detected 1 new suspicious domains:");
        assert_eq!(message.attachments.len(), 1);
        let attachment = &message.attachments[0];
        assert_eq!(attachment.fields.len(), 3);
        assert_eq!(attachment.fields[0].value, "evil.com");
        assert_eq!(attachment.fields[1].value, "phishing");
        assert_eq!(attachment.fields[2].value, "CLOUDFLARE, INC.");
    }

    #[test]
    fn test_build_slack_message_multiple_alerts() {
        let alert1 = create_test_alert("bad.net", "malware", false, None);
        let alert2 = create_test_alert("phish.org", "typo", true, Some("GOOGLE, LLC"));
        let message = build_slack_message(&[alert1, alert2]);

        assert_eq!(message.text.unwrap(), "CertWatch detected 2 new suspicious domains:");
        assert_eq!(message.attachments.len(), 2);

        // Check first alert
        assert_eq!(message.attachments[0].fields.len(), 2);
        assert_eq!(message.attachments[0].fields[0].value, "bad.net");

        // Check second alert
        assert_eq!(message.attachments[1].fields.len(), 4);
        assert_eq!(message.attachments[1].fields[0].value, "phish.org");
        assert_eq!(message.attachments[1].fields[2].value, "Newly Resolved");
        assert_eq!(message.attachments[1].fields[3].value, "GOOGLE, LLC");
    }

    #[test]
    fn test_build_slack_message_newly_resolved_flag() {
        let alert = create_test_alert("was-nx.com", "phishing", true, None);
        let message = build_slack_message(&[alert]);

        let attachment = &message.attachments[0];
        assert_eq!(attachment.fields.len(), 3);
        assert_eq!(attachment.fields[2].title, "Status");
        assert_eq!(attachment.fields[2].value, "Newly Resolved");
    }

    #[test]
    fn test_build_slack_message_no_enrichment() {
        let alert = create_test_alert("plain.com", "generic", false, None);
        let message = build_slack_message(&[alert]);

        let attachment = &message.attachments[0];
        assert_eq!(attachment.fields.len(), 2); // Domain and Source only
    }
}

use async_trait::async_trait;

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
}

impl SlackClient {
    /// Creates a new `SlackClient`.
    ///
    /// # Arguments
    /// * `webhook_url` - The Slack incoming webhook URL.
    pub fn new(webhook_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            webhook_url,
        }
    }
}

#[async_trait]
impl SlackClientTrait for SlackClient {
    /// Sends a batch of alerts to the configured Slack webhook.
    ///
    /// # Arguments
    /// * `alerts` - A slice of `Alert`s to send.
    async fn send_batch(&self, alerts: &[crate::core::Alert]) -> anyhow::Result<()> {
        if alerts.is_empty() {
            return Ok(());
        }

        let message = build_slack_message(alerts);
        self.client
            .post(&self.webhook_url)
            .json(&message)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod slack_client_tests {
    use super::*;
    use crate::core::Alert;
    use wiremock::matchers::{method, path};
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
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = SlackClient::new(format!("{}/webhook", server.uri()));
        let alerts = vec![create_test_alert("test.com")];

        // Act
        let result = client.send_batch(&alerts).await;

        // Assert
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_slack_client_handles_server_error() {
        // Arrange
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = SlackClient::new(format!("{}/webhook", server.uri()));
        let alerts = vec![create_test_alert("test.com")];

        // Act
        let result = client.send_batch(&alerts).await;

        // Assert
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_slack_client_handles_timeout() {
        // Arrange
        let server = MockServer::start().await;
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
        };
        let alerts = vec![create_test_alert("test.com")];

        // Act
        let result = client.send_batch(&alerts).await;

        // Assert
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.downcast_ref::<reqwest::Error>().unwrap().is_timeout());
    }
}