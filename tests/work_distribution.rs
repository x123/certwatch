use async_trait::async_trait;
use certwatch::core::{DnsInfo, DnsResolver};
use certwatch::dns::DnsError;
use std::{
    collections::HashMap,
    fs::File,
    io::Write,
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};
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

fn create_rule_file(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("rules.yml");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "{}", content).unwrap();
    std::mem::forget(dir);
    file_path
}

#[tokio::test]
async fn test_domain_is_processed_by_one_worker() {
    let resolver = SpyingDnsResolver::default();
    let counting_output = Arc::new(CountingOutput::new());

    let (mut test_app, app_future) = TestAppBuilder::new()
        .with_dns_resolver(Arc::new(resolver.clone()))
        .with_outputs(vec![counting_output.clone()])
        .with_config_modifier(|c| {
            c.concurrency = 4; // Use multiple workers
            c.rules.rule_files = vec![create_rule_file(
                r#"
rules:
  - name: "Test Domain Matcher"
    all:
      - domain_regex: "^test-domain-.*\\.com$"
"#,
            )];
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