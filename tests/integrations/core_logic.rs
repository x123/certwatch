use anyhow::Result;
use futures::stream::StreamExt;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Barrier;
use tokio_stream;

#[tokio::test(start_paused = true)]
async fn test_bounded_concurrency() -> Result<()> {
    let concurrency_limit = 2;
    let total_domains = 10;

    let active_tasks = Arc::new(AtomicUsize::new(0));
    let max_concurrent_tasks = Arc::new(AtomicUsize::new(0));
    let processed_domains = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(concurrency_limit));

    let domains: Vec<String> = (0..total_domains)
        .map(|i| format!("domain{}.com", i))
        .collect();

    let stream = tokio_stream::iter(domains);

    stream
        .for_each_concurrent(concurrency_limit, |_domain| {
            let active_tasks = active_tasks.clone();
            let max_concurrent_tasks = max_concurrent_tasks.clone();
            let processed_domains = processed_domains.clone();
            let barrier = barrier.clone();

            async move {
                let current_active = active_tasks.fetch_add(1, Ordering::SeqCst) + 1;
                max_concurrent_tasks.fetch_max(current_active, Ordering::SeqCst);

                // Wait for all concurrent tasks to start before proceeding.
                barrier.wait().await;

                // Simulate some async work. Under a paused clock, this is instant.
                tokio::time::sleep(Duration::from_secs(1)).await;

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