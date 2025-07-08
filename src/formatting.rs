// src/formatting.rs

use crate::core::Alert;
use std::net::IpAddr;

/// A trait for formatting a batch of alerts into a single string.
pub trait TextFormatter: Send + Sync {
    fn format_batch(&self, alerts: &[Alert]) -> String;
}

/// A formatter for Slack that creates a rich, readable, and actionable message.
pub struct SlackTextFormatter;

impl SlackTextFormatter {
    /// Extracts the base registrable domain from a full domain string.
    /// Falls back to the original domain if parsing fails.
    fn get_base_domain<'a>(&self, domain: &'a str) -> &'a str {
        let Some(domain) = psl::domain_str(domain) else {
            return domain;
        };
        domain.as_ref()
    }

    fn format_line(&self, alert: &Alert) -> String {
        let base_domain = self.get_base_domain(&alert.domain);

        // Domain part
        let domain_urlscan_link = format!(
            "<https://urlscan.io/search/#page.domain%3A{}|{}>",
            base_domain, alert.domain
        );
        let other_domain_links = format!(
            "(<https://www.shodan.io/search?query=hostname%3A{0}|shodan>|<https://www.virustotal.com/gui/domain/{0}|vt>)",
            base_domain
        );
        let domain_part = format!("{} {}", domain_urlscan_link, other_domain_links);

        // IP part
        let all_ips: Vec<_> = alert.dns.a_records.iter().chain(alert.dns.aaaa_records.iter()).collect();
        let ip_part = if !all_ips.is_empty() {
            const IP_LIMIT: usize = 3;
            let ipv4_addrs: Vec<_> = all_ips.iter().filter(|ip| ip.is_ipv4()).collect();
            let search_ips: Vec<IpAddr> = if !ipv4_addrs.is_empty() {
                ipv4_addrs.iter().take(IP_LIMIT).map(|ip| ***ip).collect()
            } else {
                all_ips.iter().take(IP_LIMIT).map(|ip| **ip).collect()
            };

            let urlscan_ip_query = search_ips
                .iter()
                .map(|ip| format!("%22{}%22", ip))
                .collect::<Vec<_>>()
                .join("%20OR%20");

            let ip_urlscan_link_text = if all_ips.len() > 1 {
                format!("{} (+{} others)", all_ips[0], all_ips.len() - 1)
            } else {
                all_ips[0].to_string()
            };

            let ip_urlscan_link = format!(
                "<https://urlscan.io/search/#page.ip%3A({})|{}>",
                urlscan_ip_query, ip_urlscan_link_text
            );

            let vt_ip_query = search_ips
                .iter()
                .map(|ip| ip.to_string())
                .collect::<Vec<_>>()
                .join("%250A");

            let other_ip_links = format!(
                "(<https://www.shodan.io/search?query=ip%3A{0}|shodan>|<https://www.virustotal.com/gui/search/{1}?type=ips|vt>)",
                all_ips[0], vt_ip_query
            );
            format!(" | {} {}", ip_urlscan_link, other_ip_links)
        } else {
            "".to_string()
        };

        // Enrichment part
        let enrichment_part = if let Some(Some(info)) = alert.enrichment.first().map(|e| e.asn_info.as_ref()) {
            let country_code = info.country_code.as_deref().unwrap_or("??");
            let asn_org = &info.as_name;
            format!(" @ {}, {}", asn_org, country_code)
        } else {
            "".to_string()
        };

        format!("{}{}{}", domain_part, ip_part, enrichment_part)
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Alert, AsnInfo, DnsInfo, EnrichmentInfo};
    use std::net::IpAddr;

    fn create_test_alert(
        domain: &str,
        ips: Vec<&str>,
        ns: Vec<&str>,
        enrichment_details: Option<(&str, u32, &str)>, // (country, asn, org)
    ) -> Alert {
        let mut dns = DnsInfo {
            a_records: ips.iter().filter_map(|s| s.parse::<IpAddr>().ok()).collect(),
            aaaa_records: vec![], // Assuming only A records for simplicity in tests
            ns_records: ns.iter().map(|s| s.to_string()).collect(),
        };
        dns.a_records.sort();

        let enrichment = if let Some((country, asn, org)) = enrichment_details {
            vec![EnrichmentInfo {
                ip: ips.first().and_then(|s| s.parse().ok()).unwrap(),
                asn_info: Some(AsnInfo {
                    as_number: asn,
                    as_name: org.to_string(),
                    country_code: Some(country.to_string()),
                }),
            }]
        } else {
            vec![]
        };

        Alert {
            timestamp: "2025-07-08T21:03:52+0200".to_string(),
            domain: domain.to_string(),
            source_tag: "phishing".to_string(), // Source tag is no longer displayed, but still part of the alert
            resolved_after_nxdomain: false,
            dns,
            enrichment,
        }
    }

    #[test]
    fn test_format_line_full_info() {
        let alert = create_test_alert(
            "com-etcapo.vip",
            vec!["104.21.86.60", "1.1.1.1", "2.2.2.2", "3.3.3.3"],
            vec!["hasslo.ns.cloudflare.com", "ophelia.ns.cloudflare.com", "ns3.com", "ns4.com"],
            Some(("US", 13335, "CLOUDFLARENET")),
        );
        let formatter = SlackTextFormatter;
        let line = formatter.format_line(&alert);

        let expected = "<https://urlscan.io/search/#page.domain%3Acom-etcapo.vip|com-etcapo.vip> (<https://www.shodan.io/search?query=hostname%3Acom-etcapo.vip|shodan>|<https://www.virustotal.com/gui/domain/com-etcapo.vip|vt>) | <https://urlscan.io/search/#page.ip%3A(%221.1.1.1%22%20OR%20%222.2.2.2%22%20OR%20%223.3.3.3%22)|1.1.1.1 (+3 others)> (<https://www.shodan.io/search?query=ip%3A1.1.1.1|shodan>|<https://www.virustotal.com/gui/search/1.1.1.1%250A2.2.2.2%250A3.3.3.3?type=ips|vt>) @ CLOUDFLARENET, US";
        assert_eq!(line, expected);
    }

    #[test]
    fn test_format_line_no_ips() {
        let alert = create_test_alert(
            "com-xxtfr.vip",
            vec![],
            vec!["ns1.example.com"],
            None,
        );
        let formatter = SlackTextFormatter;
        let line = formatter.format_line(&alert);

        let expected = "<https://urlscan.io/search/#page.domain%3Acom-xxtfr.vip|com-xxtfr.vip> (<https://www.shodan.io/search?query=hostname%3Acom-xxtfr.vip|shodan>|<https://www.virustotal.com/gui/domain/com-xxtfr.vip|vt>)";
        assert_eq!(line, expected);
    }

    #[test]
    fn test_format_line_base_domain_extraction() {
        let alert = create_test_alert(
            "ww25.hostmaster.hostmaster.icloud.com-locateiphone.us",
            vec!["199.59.243.228"],
            vec![],
            Some(("US", 16509, "AMAZON-02")),
        );
        let formatter = SlackTextFormatter;
        let line = formatter.format_line(&alert);

        let expected_domain_link = "<https://urlscan.io/search/#page.domain%3Acom-locateiphone.us|ww25.hostmaster.hostmaster.icloud.com-locateiphone.us>";
        let expected_other_links = "(<https://www.shodan.io/search?query=hostname%3Acom-locateiphone.us|shodan>|<https://www.virustotal.com/gui/domain/com-locateiphone.us|vt>)";
        assert!(line.starts_with(expected_domain_link));
        assert!(line.contains(expected_other_links));
    }

    #[test]
    fn test_format_batch() {
        let alert1 = create_test_alert("site1.com", vec!["1.1.1.1"], vec!["ns1.site1.com"], Some(("AU", 1, "APNIC")));
        let alert2 = create_test_alert("site2.com", vec!["8.8.8.8"], vec![], None);
        let formatter = SlackTextFormatter;
        let batch = formatter.format_batch(&[alert1, alert2]);

        let expected_line1 = "<https://urlscan.io/search/#page.domain%3Asite1.com|site1.com> (<https://www.shodan.io/search?query=hostname%3Asite1.com|shodan>|<https://www.virustotal.com/gui/domain/site1.com|vt>) | <https://urlscan.io/search/#page.ip%3A(%221.1.1.1%22)|1.1.1.1> (<https://www.shodan.io/search?query=ip%3A1.1.1.1|shodan>|<https://www.virustotal.com/gui/search/1.1.1.1?type=ips|vt>) @ APNIC, AU";
        let expected_line2 = "<https://urlscan.io/search/#page.domain%3Asite2.com|site2.com> (<https://www.shodan.io/search?query=hostname%3Asite2.com|shodan>|<https://www.virustotal.com/gui/domain/site2.com|vt>) | <https://urlscan.io/search/#page.ip%3A(%228.8.8.8%22)|8.8.8.8> (<https://www.shodan.io/search?query=ip%3A8.8.8.8|shodan>|<https://www.virustotal.com/gui/search/8.8.8.8?type=ips|vt>)";
        let expected = format!("```\n{}\n{}\n```", expected_line1, expected_line2);

        assert_eq!(batch, expected);
    }

}