// src/formatting.rs

use crate::core::Alert;

/// A trait for formatting a batch of alerts into a single string.
pub trait TextFormatter: Send + Sync {
    fn format_batch(&self, alerts: &[Alert]) -> String;
}

/// A formatter for Slack that creates a rich, readable, and actionable message.
pub struct SlackTextFormatter;

impl SlackTextFormatter {
    fn format_line(&self, alert: &Alert) -> String {
        let domain_link = format!(
            "<https://urlscan.io/search/#page.domain%3A{}|{}>",
            alert.domain, alert.domain
        );

        let ip_part = if let Some(first_ip) = alert.enrichment.first().map(|e| e.ip) {
            let ip_link =
                format!("<https://urlscan.io/search/#page.ip%3A{}|{}>", first_ip, first_ip);
            let other_ips_count = alert.enrichment.len() - 1;
            if other_ips_count > 0 {
                format!("{} (+{} other IPs)", ip_link, other_ips_count)
            } else {
                ip_link
            }
        } else {
            "No IP info".to_string()
        };

        let enrichment_part = if let Some(Some(info)) = alert.enrichment.first().map(|e| e.asn_info.as_ref()) {
            let country_code = info.country_code.as_deref().unwrap_or("??");
            let flag = country_code_to_flag(country_code).unwrap_or_default();
            let asn = info.as_number.to_string();
            let asn_org = &info.as_name;
            format!("[{} {}, {}, {}]", country_code, flag, asn, asn_org)
        } else {
            "[No enrichment data]".to_string()
        };

        format!(
            "[{}] {} -> {} {} [{}]",
            alert.source_tag, domain_link, ip_part, enrichment_part, alert.timestamp
        )
    }
}

impl TextFormatter for SlackTextFormatter {
    fn format_batch(&self, alerts: &[Alert]) -> String {
        if alerts.is_empty() {
            return String::new();
        }

        let lines: Vec<String> = alerts.iter().map(|alert| self.format_line(alert)).collect();

        format!("```\n{}\n```", lines.join("\n"))
    }
}

/// Converts a two-letter country code to a Slack flag emoji.
fn country_code_to_flag(code: &str) -> Option<String> {
    if code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()) {
        let emoji = format!(":flag-{}:", code.to_lowercase());
        Some(emoji)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Alert, AsnInfo, DnsInfo, EnrichmentInfo};

    fn create_test_alert(
        domain: &str,
        source: &str,
        ips: Vec<(&str, &str, u32, &str)>, // (ip, country, asn, org)
    ) -> Alert {
        let enrichment = ips
            .into_iter()
            .map(|(ip_str, country, asn, org)| EnrichmentInfo {
                ip: ip_str.parse().unwrap(),
                asn_info: Some(AsnInfo {
                    as_number: asn,
                    as_name: org.to_string(),
                    country_code: Some(country.to_string()),
                }),
            })
            .collect();

        Alert {
            timestamp: "2025-07-08T21:03:52+0200".to_string(),
            domain: domain.to_string(),
            source_tag: source.to_string(),
            resolved_after_nxdomain: false,
            dns: DnsInfo::default(),
            enrichment,
        }
    }

    #[test]
    fn test_format_line_single_ip() {
        let alert = create_test_alert(
            "65808.cc",
            "amazon",
            vec![("154.26.215.74", "US", 8796, "FD-298-8796")],
        );
        let formatter = SlackTextFormatter;
        let line = formatter.format_line(&alert);

        let expected = "[amazon] <https://urlscan.io/search/#page.domain%3A65808.cc|65808.cc> -> <https://urlscan.io/search/#page.ip%3A154.26.215.74|154.26.215.74> [US :flag-us:, 8796, FD-298-8796] [2025-07-08T21:03:52+0200]";
        assert_eq!(line, expected);
    }

    #[test]
    fn test_format_line_multiple_ips() {
        let alert = create_test_alert(
            "693564.cc",
            "amazon",
            vec![
                ("188.114.96.1", "US", 13335, "CLOUDFLARENET"),
                ("1.1.1.1", "AU", 1, "APNIC"),
                ("8.8.8.8", "US", 8, "GOOGLE"),
            ],
        );
        let formatter = SlackTextFormatter;
        let line = formatter.format_line(&alert);

        let expected = "[amazon] <https://urlscan.io/search/#page.domain%3A693564.cc|693564.cc> -> <https://urlscan.io/search/#page.ip%3A188.114.96.1|188.114.96.1> (+2 other IPs) [US :flag-us:, 13335, CLOUDFLARENET] [2025-07-08T21:03:52+0200]";
        assert_eq!(line, expected);
    }

    #[test]
    fn test_format_line_missing_enrichment() {
        let mut alert = create_test_alert("no-enrich.com", "generic", vec![]);
        alert.enrichment = vec![EnrichmentInfo {
            ip: "0.0.0.0".parse().unwrap(),
            asn_info: None,
        }];
        let formatter = SlackTextFormatter;
        let line = formatter.format_line(&alert);

        let expected = "[generic] <https://urlscan.io/search/#page.domain%3Ano-enrich.com|no-enrich.com> -> <https://urlscan.io/search/#page.ip%3A0.0.0.0|0.0.0.0> [No enrichment data] [2025-07-08T21:03:52+0200]";
        assert_eq!(line, expected);
    }

    #[test]
    fn test_format_batch() {
        let alert1 = create_test_alert(
            "site1.com",
            "p1",
            vec![("1.1.1.1", "AU", 1, "APNIC")],
        );
        let alert2 = create_test_alert(
            "site2.com",
            "p2",
            vec![("8.8.8.8", "US", 8, "GOOGLE")],
        );
        let formatter = SlackTextFormatter;
        let batch = formatter.format_batch(&[alert1, alert2]);

        let expected_line1 = "[p1] <https://urlscan.io/search/#page.domain%3Asite1.com|site1.com> -> <https://urlscan.io/search/#page.ip%3A1.1.1.1|1.1.1.1> [AU :flag-au:, 1, APNIC] [2025-07-08T21:03:52+0200]";
        let expected_line2 = "[p2] <https://urlscan.io/search/#page.domain%3Asite2.com|site2.com> -> <https://urlscan.io/search/#page.ip%3A8.8.8.8|8.8.8.8> [US :flag-us:, 8, GOOGLE] [2025-07-08T21:03:52+0200]";
        let expected = format!("```\n{}\n{}\n```", expected_line1, expected_line2);

        assert_eq!(batch, expected);
    }

    #[test]
    fn test_country_code_to_flag() {
        assert_eq!(country_code_to_flag("US"), Some(":flag-us:".to_string()));
        assert_eq!(country_code_to_flag("GB"), Some(":flag-gb:".to_string()));
        assert_eq!(country_code_to_flag("de"), Some(":flag-de:".to_string()));
        assert_eq!(country_code_to_flag("USA"), None);
        assert_eq!(country_code_to_flag(""), None);
        assert_eq!(country_code_to_flag("12"), None);
    }
}