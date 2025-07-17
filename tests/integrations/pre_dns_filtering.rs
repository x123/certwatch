use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
#[path = "../helpers/mod.rs"]
mod helpers;
use helpers::app::TestAppBuilder;
use helpers::mock_dns::MockDnsResolver;

#[tokio::test]
#[ignore]
async fn test_domain_is_filtered_before_dns() -> Result<()> {
    // 1. Setup: Create a mock DNS resolver that will panic if called.
    let dns_resolver = Arc::new(
        MockDnsResolver::new()
            .with_panic_on_resolve()
            .with_allowed_domain("google.com") // Allow google.com for DNS health check
            .with_allowed_domain("another-domain.com"),
    );

    // 2. Setup: Define rules for ignoring, whitelisting, and dropping domains.
    let rules = r#"
ignore:
  - "^ignore-me\\.com$"
rules:
  - name: "whitelist-rule"
    domain_regex: "^another-domain\\.com$"
"#;

    // 3. Setup: Initialize the TestApp with the panicking resolver and the rules.
    let test_app = TestAppBuilder::new()
        .with_test_domains_channel()
        .with_dns_resolver(dns_resolver)
        .with_metrics() // Enable metrics
        .with_rules(rules)
        .start()
        .await?; // Await the result of start

    // 4. Act: Send one domain to be ignored, one to be whitelisted, and one to be dropped.
    test_app.send_domain("ignore-me.com").await?;
    test_app.send_domain("another-domain.com").await?;
    test_app.send_domain("drop-me.com").await?;

    // 5. Assert: Give the app a moment to process, then check the metrics.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let metrics_body = reqwest::get(&format!("http://{}/metrics", test_app.metrics_addr().unwrap()))
        .await?
        .text()
        .await?;

    assert!(
        metrics_body.contains(r#"domains_ignored_total 1"#),
        "Metric 'domains_ignored_total' should be 1. Body was:\n{}",
        metrics_body
    );

    assert!(
        metrics_body.contains(r#"certwatch_domains_prefiltered_total 1"#),
        "Metric 'certwatch_domains_prefiltered_total' should be 1. Body was:\n{}",
        metrics_body
    );

    // 6. Teardown
    test_app.shutdown().await?;

    Ok(())
}