//! Unit test for enrichment failure handling.

use anyhow::Result;
use certwatch::{
    internal_metrics::Metrics,
    app::process_domain,
    config::RulesConfig,
    core::DnsInfo,
    dns::{test_utils::FakeDnsResolver, DnsHealthMonitor, DnsResolutionManager},
    enrichment::fake::FakeEnrichmentProvider,
    rules::{RuleLoader, RuleMatcher},
};
use std::{fs::File, io::Write, path::PathBuf, sync::Arc};
use tempfile::tempdir;
use tokio::sync::{broadcast, watch};

/// Creates a temporary YAML rule file and returns its path.
fn create_rule_file(content: &str) -> PathBuf {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("rules.yml");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "{}", content).unwrap();
    // The tempdir is intentionally leaked here to prevent the file from being deleted.
    std::mem::forget(dir);
    file_path
}

#[tokio::test]
async fn test_process_domain_propagates_enrichment_error() -> Result<()> {
    // 1. Arrange
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let dns_resolver = Arc::new(FakeDnsResolver::new());
    let mut dns_info = DnsInfo::default();
    let error_ip = "1.2.3.4".parse().unwrap();
    dns_info.a_records.push(error_ip);
    dns_resolver.add_success_response("example.com", dns_info);

    let health_monitor = DnsHealthMonitor::new(
        Default::default(),
        dns_resolver.clone(),
        shutdown_rx.clone(),
        Arc::new(Metrics::new_for_test()),
        vec![],
    );
    let (dns_manager, _) = DnsResolutionManager::new(
        dns_resolver.clone(),
        Default::default(),
        health_monitor,
        shutdown_rx,
        Arc::new(Metrics::new_for_test()),
    );
    let dns_manager = Arc::new(dns_manager);

    // Configure the enrichment provider to fail for the specific IP
    let enrichment_provider = FakeEnrichmentProvider::new();
    enrichment_provider.add_error(error_ip, "Enrichment database offline");
    let enrichment_provider = Arc::new(enrichment_provider);

    let (alerts_tx, _) = broadcast::channel(1);

    // 2. Act
    let result = process_domain(
        "example.com".to_string(),
        0, // worker_id
        Arc::new(
            RuleMatcher::new(
                RuleLoader::load_from_files(&RulesConfig {
                    rule_files: Some(vec![create_rule_file(
                        r#"
rules:
    - name: "Always Match"
      all:
        - domain_regex: ".*"
"#,
                    )]),
                })
                .unwrap(),
                Arc::new(Metrics::new_for_test()),
            )
            .unwrap(),
        ),
        dns_manager,
        enrichment_provider,
        alerts_tx,
    )
    .await;

    // 3. Assert
    assert!(result.is_err(), "Expected process_domain to return an error");
    let err_string = result.unwrap_err().to_string();
    assert!(
        err_string.contains("Failed to build alert"),
        "Error message did not contain expected text"
    );

    // ensure shutdown channel is used
    drop(shutdown_tx);
    Ok(())
}