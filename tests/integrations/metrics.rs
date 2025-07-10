#[path = "../helpers/mod.rs"]
mod helpers;

use anyhow::Result;
use certwatch::internal_metrics::Metrics;
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver};

#[tokio::test]
async fn metrics_endpoint_is_available() -> Result<()> {
    let test_app = TestAppBuilder::new().with_metrics().start().await?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{}/metrics", test_app.metrics_addr()))
        .send()
        .await?;

    assert!(response.status().is_success());
    let body = response.text().await?;
    assert!(body.contains("process_cpu_usage_percent"));

    Ok(())
}

#[tokio::test]
async fn test_domain_counting_metrics() -> Result<()> {
    let metrics = Arc::new(Metrics::new_for_test());
    let mock_dns = Arc::new(MockDnsResolver::new(metrics));
    mock_dns.add_response("google.com", Ok(Default::default())); // Health check
    mock_dns.add_response("matching.com", Ok(Default::default())); // Test domain

    let builder = TestAppBuilder::new()
        .with_dns_resolver(mock_dns)
        .with_metrics()
        .with_test_domains_channel();

    let test_app = builder
        .with_rules("rules:\n  - name: test-rule\n    domain_regex: 'matching.com'")
        .await
        .start()
        .await?;

    let client = reqwest::Client::new();

    test_app.send_domain("matching.com").await?;

    // Wait for the metrics to be updated
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let body = client
        .get(format!("http://{}/metrics", test_app.metrics_addr()))
        .send()
        .await?
        .text()
        .await?;

    assert!(
        body.contains("domains_processed_total 1"),
        "Metric 'domains_processed_total' not found or incorrect."
    );
    assert!(
        body.contains("rule_matches_total{rule=\"test-rule\"} 1"),
        "Metric 'rule_matches_total' not found or incorrect. Body: {}",
        body
    );

    Ok(())
}

use std::sync::Arc;

#[tokio::test]
async fn dns_failure_metric_is_incremented() -> Result<()> {
    // We use a non-routable address to guarantee a DNS timeout/failure.
    let invalid_resolver = "10.255.255.1:53";

    let builder = TestAppBuilder::new()
        .with_metrics()
        .with_test_domains_channel()
        .with_disabled_periodic_health_check()
        .with_config_modifier(|c| {
            c.dns.resolver = Some(invalid_resolver.to_string());
            c.dns.timeout_ms = 100;
            c.dns.retry_config.retries = Some(1);
        });

    let test_app = builder
        .with_skipped_health_check()
        .with_rules("rules:\n  - name: test-rule\n    domain_regex: 'failing-domain.com'")
        .await
        .start()
        .await?;

    let client = reqwest::Client::new();

    test_app.send_domain("failing-domain.com").await?;

    let mut body = String::new();
    // Wait for the resolution to fail and the metric to be updated.
    // This might take a few moments due to the DNS timeout.
    for _ in 0..20 {
        let response = client
            .get(format!("http://{}/metrics", test_app.metrics_addr()))
            .send()
            .await?;
        body = response.text().await?;
        // With one retry, we expect 2 failures total.
        if body.contains("dns_queries_total{status=\"failure\"} 2") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    assert!(
        body.contains("dns_queries_total{status=\"failure\"} 2"),
        "Metric 'dns_queries_total' with status 'failure' not found or incorrect. Body:\n{}",
        body
    );

    Ok(())
}
#[tokio::test]
async fn metrics_format_is_correct() -> Result<()> {
    let test_app = TestAppBuilder::new().with_metrics().start().await?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("http://{}/metrics", test_app.metrics_addr()))
        .send()
        .await?;

    assert!(response.status().is_success());
    let body = response.text().await?;

    let lines: Vec<&str> = body.lines().collect();
    let metric_name = "process_cpu_usage_percent";

    let metric_line_index = lines
        .iter()
        .position(|&line| line.starts_with(metric_name))
        .unwrap_or_else(|| panic!("Metric '{}' not found in output", metric_name));

    assert!(metric_line_index >= 2, "Metric '{}' should be preceded by HELP and TYPE lines", metric_name);

    let type_line = lines[metric_line_index - 1];
    let help_line = lines[metric_line_index - 2];

    assert!(
        type_line.starts_with(&format!("# TYPE {} gauge", metric_name)),
        "TYPE line is missing or incorrect for metric '{}'. Found: {}",
        metric_name,
        type_line
    );

    assert!(
        help_line.starts_with(&format!("# HELP {}", metric_name)),
        "HELP line is missing or incorrect for metric '{}'. Found: {}",
        metric_name,
        help_line
    );

    Ok(())
}