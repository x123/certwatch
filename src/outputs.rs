use crate::core::Alert;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Output: Send + Sync {
    async fn send_alert(&self, alert: &Alert) -> Result<()>;
}

pub struct OutputManager {
    outputs: Vec<Box<dyn Output>>,
}

impl OutputManager {
    pub fn new(outputs: Vec<Box<dyn Output>>) -> Self {
        Self { outputs }
    }

    pub async fn send_alert(&self, alert: &Alert) -> Result<()> {
        for output in &self.outputs {
            output.send_alert(alert).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AsnData, DnsInfo, EnrichmentInfo, GeoIpInfo};
    use std::io::Write;
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

    // Test for StdoutOutput
    pub struct StdoutOutput {
        writer: Arc<Mutex<Vec<u8>>>,
    }

    impl StdoutOutput {
        fn new(writer: Arc<Mutex<Vec<u8>>>) -> Self {
            Self { writer }
        }
    }

    #[async_trait]
    impl Output for StdoutOutput {
        async fn send_alert(&self, alert: &Alert) -> Result<()> {
            let mut writer = self.writer.lock().unwrap();
            writeln!(&mut *writer, "{}", serde_json::to_string(alert)?)?;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_stdout_output() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let output = StdoutOutput::new(buffer.clone());
        let alert = create_test_alert();

        output.send_alert(&alert).await.unwrap();

        let output_data = buffer.lock().unwrap();
        let output_string = String::from_utf8(output_data.to_vec()).unwrap();
        let expected_json = serde_json::to_string(&alert).unwrap() + "\n";

        assert_eq!(output_string, expected_json);
    }

    // Test for JsonOutput
    pub struct JsonOutput {
        writer: Arc<Mutex<Vec<u8>>>,
    }

    impl JsonOutput {
        fn new(writer: Arc<Mutex<Vec<u8>>>) -> Self {
            Self { writer }
        }
    }

    #[async_trait]
    impl Output for JsonOutput {
        async fn send_alert(&self, alert: &Alert) -> Result<()> {
            let mut writer = self.writer.lock().unwrap();
            serde_json::to_writer(&mut *writer, alert)?;
            writeln!(writer)?;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_json_output() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let output = JsonOutput::new(buffer.clone());
        let alert = create_test_alert();

        output.send_alert(&alert).await.unwrap();

        let output_data = buffer.lock().unwrap();
        let output_string = String::from_utf8(output_data.to_vec()).unwrap();
        let expected_json = serde_json::to_string(&alert).unwrap() + "\n";

        assert_eq!(output_string, expected_json);
    }

    // Slack Output
    pub struct SlackOutput {
        client: reqwest::Client,
        webhook_url: String,
    }

    impl SlackOutput {
        fn new(webhook_url: String) -> Self {
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
                "text": format!("New suspicious domain found: {}", alert.domain),
                "blocks": [
                    {
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": format!("*Domain:* {}\n*Source:* {}", alert.domain, alert.source_tag)
                        }
                    }
                ]
            });

            self.client
                .post(&self.webhook_url)
                .json(&payload)
                .send()
                .await?
                .error_for_status()?;

            Ok(())
        }
    }

    #[tokio::test]
    async fn test_slack_output() {
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
