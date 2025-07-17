use certwatch::core::DnsInfo;
use std::{sync::Arc, time::Duration};
use tokio::sync::oneshot;
use tracing::{debug, info, instrument};

// Import test helpers from the `tests` module.
#[path = "../helpers/mod.rs"]
mod helpers;
use certwatch::internal_metrics::Metrics;
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver, mock_output::CountingOutput};

#[tokio::test]
#[instrument(skip_all)]
async fn test_domain_is_processed_by_one_worker() {
    let metrics = Arc::new(Metrics::new_for_test());
    let resolver = Arc::new(MockDnsResolver::new_with_metrics(metrics));
    // The health check will try to resolve google.com
    resolver.add_response("google.com", Ok(DnsInfo::default()));

    let domains_to_send = 10;
    let (completion_tx, completion_rx) = oneshot::channel();
    let counting_output = Arc::new(CountingOutput::new(domains_to_send).with_completion_signal(completion_tx));

    // Pre-configure the DNS resolver to successfully resolve all test domains.
    for i in 0..domains_to_send {
        let domain = format!("test-domain-{}.com", i);
        resolver.add_response(&domain, Ok(DnsInfo::default()));
    }

    let rules = r#"
rules:
  - name: "Test Domain Matcher"
    pre_dns_filter: false
    all:
      - domain_regex: "^test-domain-.*\\.com$"
"#;
    let builder = TestAppBuilder::new()
        .with_dns_resolver(resolver.clone())
        .with_outputs(vec![counting_output.clone()])
        .with_config_modifier(|c| {
            c.performance.dns_worker_concurrency = 4;
            c.performance.rules_worker_concurrency = 4;
            c.performance.queue_capacity = 100; // Increase queue capacity
        })
        .with_test_domains_channel()
        .with_rules(rules)
        .with_skipped_health_check();

    let mut test_app = builder
        .start()
        .await
        .expect("TestApp should start successfully");

    // Wait for the app to signal that it's ready
    test_app.wait_for_startup().await.unwrap();

    // Send all the domains now that the app is running.
    for i in 0..domains_to_send {
        let domain = format!("test-domain-{}.com", i);
        test_app
            .send_domain(&domain)
            .await
            .expect("Failed to send domain to worker pool");
    }
    test_app.close_domains_channel();
    test_app.close_domains_channel();

    // Wait for all domains to be processed by the output.
    tokio::select! {
        _ = completion_rx => {}
        _ = tokio::time::sleep(Duration::from_secs(10)) => {
            panic!("Test timed out waiting for CountingOutput completion signal.");
        }
    }

    // Shut down the app using the provided helper
    test_app.shutdown().await.expect("App shutdown failed");


    // Now, verify the work distribution for our test domains.
    for i in 0..domains_to_send {
        let domain = format!("test-domain-{}.com", i);
        assert_eq!(
            resolver.get_resolve_count(&domain),
            1,
            "Domain '{}' was processed {} times, but should only be processed once.",
            domain,
            resolver.get_resolve_count(&domain)
        );
    }
}