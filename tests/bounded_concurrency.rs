use anyhow::Result;
use certwatch::{
    config::Config,
    core::{DnsInfo, EnrichmentProvider, PatternMatcher},
    dns::{DnsHealthMonitor, DnsResolutionManager, DnsResolver},
};
use futures::stream::StreamExt;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_stream;

// Mock implementations for services
struct MockPatternMatcher;
#[async_trait::async_trait]
impl PatternMatcher for MockPatternMatcher {
    async fn match_domain(&self, _domain: &str) -> Option<String> {
        Some("mock_tag".to_string())
    }
}

struct MockEnrichmentProvider;
#[async_trait::async_trait]
impl EnrichmentProvider for MockEnrichmentProvider {
    async fn enrich(&self, ip: std::net::IpAddr) -> Result<certwatch::core::EnrichmentInfo> {
        Ok(certwatch::core::EnrichmentInfo {
            ip,
            data: Some(Default::default()),
        })
    }
}

struct MockDnsResolver;
#[async_trait::async_trait]
impl DnsResolver for MockDnsResolver {
    async fn resolve(&self, _domain: &str) -> Result<DnsInfo> {
        Ok(DnsInfo {
            a_records: vec!["1.1.1.1".parse()?],
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn test_bounded_concurrency() -> Result<()> {
    let concurrency_limit = 2;
    let total_domains = 100;

    let mut config = Config::default();
    config.concurrency = concurrency_limit;

    let pattern_matcher = Arc::new(MockPatternMatcher);
    let dns_resolver: Arc<dyn DnsResolver> = Arc::new(MockDnsResolver);
    let health_monitor = DnsHealthMonitor::new(Default::default(), dns_resolver.clone());
    let (dns_manager, _) =
        DnsResolutionManager::new(dns_resolver, Default::default(), health_monitor);
    let dns_manager = Arc::new(dns_manager);

    let enrichment_provider: Arc<dyn EnrichmentProvider> = Arc::new(MockEnrichmentProvider);
    let (alerts_tx, _alerts_rx) = mpsc::channel(total_domains);

    let active_tasks = Arc::new(AtomicUsize::new(0));
    let max_concurrent_tasks = Arc::new(AtomicUsize::new(0));
    let processed_domains = Arc::new(AtomicUsize::new(0));

    let domains: Vec<String> = (0..total_domains)
        .map(|i| format!("domain{}.com", i))
        .collect();

    let stream = tokio_stream::iter(domains);

    stream
        .for_each_concurrent(concurrency_limit, |domain| {
            let pattern_matcher = pattern_matcher.clone();
            let dns_manager = dns_manager.clone();
            let enrichment_provider = enrichment_provider.clone();
            let alerts_tx = alerts_tx.clone();
            let active_tasks = active_tasks.clone();
            let max_concurrent_tasks = max_concurrent_tasks.clone();
            let processed_domains = processed_domains.clone();

            async move {
                // Increment active tasks and update max
                let current_active = active_tasks.fetch_add(1, Ordering::SeqCst) + 1;
                max_concurrent_tasks.fetch_max(current_active, Ordering::SeqCst);

                // Simulate work
                tokio::time::sleep(Duration::from_millis(10)).await;

                if let Some(source_tag) = pattern_matcher.match_domain(&domain).await {
                    if let Ok(dns_info) = dns_manager.resolve_with_retry(&domain, &source_tag).await {
                        let alert = certwatch::build_alert(
                            domain,
                            source_tag,
                            false,
                            dns_info,
                            enrichment_provider,
                        )
                        .await;
                        let _ = alerts_tx.send(alert).await;
                    }
                }
                
                processed_domains.fetch_add(1, Ordering::SeqCst);
                active_tasks.fetch_sub(1, Ordering::SeqCst);
            }
        })
        .await;

    assert_eq!(
        max_concurrent_tasks.load(Ordering::SeqCst),
        concurrency_limit,
        "The number of concurrent tasks should not exceed the limit"
    );

    assert_eq!(
        processed_domains.load(Ordering::SeqCst),
        total_domains,
        "All domains should be processed"
    );

    Ok(())
}