use async_trait::async_trait;
use certwatch::core::{DnsInfo, DnsResolver, PatternMatcher};
use certwatch::dns::DnsError;
use std::collections::HashMap;
use std::sync::{atomic::Ordering, Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;

// Import test helpers from the `tests` module.
mod helpers;
use helpers::app::TestAppBuilder;
use helpers::mock_output::CountingOutput;

/// A test-only DNS resolver that spies on calls to `resolve`,
/// counting how many times each domain is resolved.
#[derive(Clone, Default)]
struct SpyingDnsResolver {
    counts: Arc<Mutex<HashMap<String, u32>>>,
}

#[async_trait]
impl DnsResolver for SpyingDnsResolver {
    async fn resolve(&self, domain: &str) -> Result<DnsInfo, DnsError> {
        let mut counts = self.counts.lock().unwrap();
        *counts.entry(domain.to_string()).or_insert(0) += 1;
        Ok(DnsInfo::default())
    }
}

/// A mock pattern matcher that always returns a match for our test domains.
#[derive(Clone)]
struct MockPatternMatcher;

#[async_trait]
impl PatternMatcher for MockPatternMatcher {
    async fn match_domain(&self, domain: &str) -> Option<String> {
        if domain.starts_with("test-domain-") {
            Some("test-match".to_string())
        } else {
            None
        }
    }
}

#[tokio::test]
async fn test_domain_is_processed_by_one_worker() {
    let resolver = SpyingDnsResolver::default();
    let counting_output = Arc::new(CountingOutput::new());
    let pattern_matcher = Arc::new(MockPatternMatcher);

    let (mut test_app, app_future) = TestAppBuilder::new()
        .with_dns_resolver(Arc::new(resolver.clone()))
        .with_outputs(vec![counting_output.clone()])
        .with_pattern_matcher(pattern_matcher)
        .with_config_modifier(|c| {
            c.concurrency = 4; // Use multiple workers
        })
        .build()
        .await
        .expect("TestApp should build successfully");

    // Spawn the application in the background.
    let app_handle = tokio::spawn(app_future);
    test_app.app_handle = Some(app_handle);

    // Send all the domains now that the app is running.
    let domains_to_send = 10;
    for i in 0..domains_to_send {
        let domain = format!("test-domain-{}.com", i);
        test_app
            .domains_tx
            .send(domain)
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

    let mut counts = resolver.counts.lock().unwrap().clone();

    // Account for the startup health check.
    let health_check_domain = "google.com";
    assert_eq!(
        counts.get(health_check_domain),
        Some(&1),
        "Startup health check should have resolved the recovery domain exactly once."
    );
    counts.remove(health_check_domain);

    // Now, verify the work distribution for our test domains.
    assert_eq!(
        counts.len(),
        domains_to_send,
        "All test domains should have been processed"
    );

    for (domain, count) in counts.iter() {
        assert_eq!(
            *count,
            1,
            "Domain '{}' was processed {} times, but should only be processed once.",
            domain,
            count
        );
    }
}