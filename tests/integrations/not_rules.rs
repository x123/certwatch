#[path = "../helpers/mod.rs"]
mod helpers;

use crate::helpers::mock_dns::MockDnsResolver;
use certwatch::core::DnsInfo;
use helpers::app::TestAppBuilder;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::test]
async fn test_not_asns_rule_does_not_match_on_enrichment_failure() {
    // Arrange
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::new("trace,certwatch::network=off"))
        .init();

    let (alerts_tx, mut alerts_rx) = broadcast::channel(100);

    let resolver = Arc::new(MockDnsResolver::new());
    resolver.add_response("google.com", Ok(DnsInfo::default())); // For health check
    resolver.add_response("example.com", Ok(DnsInfo::default()));

    // 1. Configure a rule that should only match if enrichment is successful
    let rules = r#"
rules:
  - name: "Stage 1 Pass-Through"
    domain_regex: ".*"
    pre_dns_filter: false
  - name: "Non-Google .com"
    all:
      - domain_regex: "\\.com$"
      - not_asns: [15169] # Google LLC
"#;

    // 2. Build the test application, configuring the enrichment provider to fail
    let builder = TestAppBuilder::new()
        .with_dns_resolver(resolver)
        .with_alert_tx(alerts_tx)
        .with_failing_enrichment(true)
        .with_config_modifier(|c| {
            c.performance.dns_worker_concurrency = 1;
            c.performance.rules_worker_concurrency = 1;
        })
        .with_rules(rules)
        .with_test_domains_channel();

    let test_app = builder.start().await.unwrap();
    test_app.wait_for_startup().await.unwrap();

    // 5. Act: Send a domain that matches the regex part of the rule
    test_app.send_domain("example.com").await.unwrap();

    // Give the app a moment to process the domain
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 6. Assert: Check that NO alert is received
    // With the buggy logic, the enrichment failure would cause the `not_asns`
    // to incorrectly return `true`, and an alert would be sent.
    // The correct logic should not produce an alert.
    // 6. Cleanup: Shut down the app first. This prevents a deadlock where the
    // test waits on recv() and the NotificationManager also waits on recv(),
    // preventing it from seeing the shutdown signal.
    test_app.shutdown().await.unwrap();

    // 7. Assert: Now check if any alert was sent.
    // After shutdown, try_recv() will immediately return an error if no message
    // was sent, or Ok(alert) if one is in the buffer.
    let recv_result = alerts_rx.try_recv();
    match recv_result {
        Ok(alert) => {
            panic!(
                "Expected no alert to be sent, but one was received: {:?}",
                alert
            );
        }
        Err(broadcast::error::TryRecvError::Closed) => {
            // This is the expected outcome: the channel is closed and empty.
        }
        Err(e) => {
            panic!("Received unexpected error from alerts channel: {}", e);
        }
    }
}