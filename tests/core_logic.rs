use anyhow::Result;
use certwatch::{
    config::Config,
    dns::{DnsHealthMonitor, DnsResolutionManager},
};
use futures::stream::StreamExt;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::{mpsc, watch, Barrier};
use tokio_stream;

mod helpers;
use helpers::create_mock_dependencies;

#[tokio::test]
async fn test_bounded_concurrency() -> Result<()> {
    let concurrency_limit = 2;
    let total_domains = 10;

    let mut config = Config::default();
    config.concurrency = concurrency_limit;

    let (pattern_matcher, dns_resolver, enrichment_provider) = create_mock_dependencies();
    let (_tx, rx) = watch::channel(());
    let health_monitor = DnsHealthMonitor::new(Default::default(), dns_resolver.clone(), rx.clone());
    let (dns_manager, _) =
        DnsResolutionManager::new(dns_resolver, Default::default(), health_monitor, rx);
    let dns_manager = Arc::new(dns_manager);

    let (alerts_tx, _alerts_rx) = mpsc::channel(total_domains);

    let active_tasks = Arc::new(AtomicUsize::new(0));
    let max_concurrent_tasks = Arc::new(AtomicUsize::new(0));
    let processed_domains = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(concurrency_limit));

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
            let barrier = barrier.clone();

            async move {
                let current_active = active_tasks.fetch_add(1, Ordering::SeqCst) + 1;
                max_concurrent_tasks.fetch_max(current_active, Ordering::SeqCst);

                barrier.wait().await;

                if let Some(source_tag) = pattern_matcher.match_domain(&domain).await {
                    if let Ok(dns_info) = dns_manager.resolve_with_retry(&domain, &source_tag).await
                    {
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