use certwatch::core::DnsInfo;
use std::{sync::Arc, time::Duration};

// Import test helpers from the `tests` module.
#[path = "../helpers/mod.rs"]
mod helpers;
use certwatch::internal_metrics::Metrics;
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver, mock_output::CountingOutput};

#[tokio::test]
async fn test_domain_is_processed_by_one_worker() {
    let metrics = Arc::new(Metrics::new_for_test());
    let resolver = Arc::new(MockDnsResolver::new(metrics));
    // The health check will try to resolve google.com
    resolver.add_response("google.com", Ok(DnsInfo::default()));

    let counting_output = Arc::new(CountingOutput::new());
    let domains_to_send = 10;

    // Pre-configure the DNS resolver to successfully resolve all test domains.
    for i in 0..domains_to_send {
        let domain = format!("test-domain-{}.com", i);
        resolver.add_response(&domain, Ok(DnsInfo::default()));
    }

    let rules = r#"
rules:
  - name: "Test Domain Matcher"
    all:
      - domain_regex: "^test-domain-.*\\.com$"
"#;
    let (mut test_app, app_future) = TestAppBuilder::new()
        .with_dns_resolver(resolver.clone())
        .with_outputs(vec![counting_output.clone()])
        .with_config_modifier(|c| {
            c.performance.dns_worker_concurrency = 4;
            c.performance.rules_worker_concurrency = 4;
        })
        .with_test_domains_channel()
        .with_rules(rules)
        .await
        .build()
        .await
        .expect("TestApp should build successfully");

    // Spawn the application in the background.
    let app_handle = tokio::spawn(app_future);
    test_app.app_handle = Some(app_handle);

    // Send all the domains now that the app is running.
    for i in 0..domains_to_send {
        let domain = format!("test-domain-{}.com", i);
        test_app
            .send_domain(&domain)
            .await
            .expect("Failed to send domain to worker pool");
    }

    // Wait until all domains have been processed by the workers.
    // With the async DNS manager, the worker returns immediately after handing
    // off the domain. The actual resolution and output happen in the background.
    // For this test, we'll just wait a fixed amount of time to allow the
    // processing to complete. This is not ideal, but it's a pragmatic
    // solution for this integration test.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Shut down the app using the provided helper
    test_app
        .shutdown(Duration::from_secs(5))
        .await
        .expect("App should shut down gracefully");

    // Account for the startup health check.
    assert_eq!(
        resolver.get_resolve_count("google.com"),
        1,
        "Startup health check should have resolved the recovery domain exactly once."
    );

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