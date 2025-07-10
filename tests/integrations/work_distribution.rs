use certwatch::core::DnsInfo;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::time::timeout;

// Import test helpers from the `tests` module.
#[path = "../helpers/mod.rs"]
mod helpers;
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver, mock_output::CountingOutput};

#[tokio::test]
async fn test_domain_is_processed_by_one_worker() {
    let resolver = Arc::new(MockDnsResolver::new());
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
            c.core.concurrency = 4; // Use multiple workers
        })
        .with_rules(rules)
        .await
        .with_test_domains_channel()
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
    let wait_for_completion = async {
        while counting_output.count.load(Ordering::SeqCst) < domains_to_send {
            counting_output.notifier.notified().await;
        }
    };

    timeout(Duration::from_secs(10), wait_for_completion)
        .await
        .expect("Timed out waiting for all domains to be processed");

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