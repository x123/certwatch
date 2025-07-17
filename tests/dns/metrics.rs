use crate::helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver};
use certwatch::{dns::DnsRetryConfig, internal_metrics::Metrics};
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn test_dns_retry_backoff_metric() {
    let metrics = Arc::new(Metrics::new_for_test());
    let mock_dns = Arc::new(MockDnsResolver::new_with_metrics(metrics.clone()));
    let mut resolve_attempts_rx = mock_dns.get_resolve_attempts_rx();

    // Configure the mock resolver to always fail for "fail.com"
    mock_dns.add_failure("fail.com").await;

    // Whitelist "fail.com" so it passes the pre-DNS filter.
    let rules = r#"
rules:
  - name: "allow fail.com"
    domain_regex: "^fail\\.com$"
"#;

    let test_app_builder = TestAppBuilder::new()
        .with_rules(rules)
        .with_config_modifier(|config| {
            // Use a small, predictable backoff for testing
            config.dns.retry_config = DnsRetryConfig {
                retries: Some(2),
                backoff_ms: Some(10),
                nxdomain_retries: Some(0), // Not used in this test
                nxdomain_backoff_ms: Some(0), // Not used in this test
            };
        })
        .with_dns_resolver(mock_dns)
        .with_test_domains_channel()
        .with_metrics()
        .with_skipped_health_check()
        .with_disabled_periodic_health_check();

    let test_app = test_app_builder.start().await.unwrap();
    test_app.wait_for_startup().await.unwrap();

    // Send the domain that will trigger retries
    test_app.send_domain("fail.com").await.unwrap();

    // The initial attempt + 2 retries = 3 total attempts.
    // We need to wait for all retries to complete before checking metrics.
    for i in 1..=3 {
        match tokio::time::timeout(Duration::from_secs(1), resolve_attempts_rx.recv()).await {
            Ok(Some(domain)) => assert_eq!(domain, "fail.com"),
            _ => panic!("Did not receive resolve attempt {} in time", i),
        }
    }

    // Fetch metrics via HTTP and parse the response
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{}/metrics", test_app.metrics_addr().unwrap()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    // There should be 2 sleep samples recorded (one after the first failure, one after the second)
    let count_line = response
        .lines()
        .find(|line| line.starts_with("dns_retry_backoff_delay_seconds_count"))
        .expect("Metric 'dns_retry_backoff_delay_seconds_count' not found");
    let count: u64 = count_line.split_whitespace().last().unwrap().parse().unwrap();
    assert_eq!(count, 2, "Expected 2 backoff delay samples for 2 retries");

    // The backoff is 10ms * 2^attempt, where attempt is 0-indexed.
    // 1st retry (attempt 0): 10 * 2^0 = 10ms
    // 2nd retry (attempt 1): 10 * 2^1 = 20ms
    // Total sleep time = 10ms + 20ms = 30ms = 0.03s
    let sum_line = response
        .lines()
        .find(|line| line.starts_with("dns_retry_backoff_delay_seconds_sum"))
        .expect("Metric 'dns_retry_backoff_delay_seconds_sum' not found");
    let sum: f64 = sum_line.split_whitespace().last().unwrap().parse().unwrap();

    // Allow for a small delta due to timing variations
    assert!(
        (sum - 0.03).abs() < 0.01,
        "Sum of backoff delays was {:.4}s, expected ~0.03s",
        sum
    );

    test_app.shutdown().await.unwrap();
}