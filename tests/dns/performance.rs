use crate::helpers::{
    app::TestAppBuilder,
    mock_dns::MockDnsResolver,
    mock_output::CountingOutput,
};
use certwatch::config::PerformanceConfig;
use std::{sync::Arc, time::Duration};
use tokio::{sync::oneshot, time::Instant};
use tracing::info;

#[tokio::test]
async fn test_high_volume_dns_resolution_performance() {
    let total_domains = 1_000;
    let worker_concurrency = 8;
    let resolution_delay = Duration::from_millis(10);

    let metrics = Arc::new(certwatch::internal_metrics::Metrics::new_for_test());
    let mock_resolver = Arc::new(MockDnsResolver::new_with_metrics(metrics.clone()).with_delay(resolution_delay));

    let (completion_tx, completion_rx) = oneshot::channel();
    let output = Arc::new(CountingOutput::new(total_domains).with_completion_signal(completion_tx));
    let catch_all_rule = r#"
rules:
  - name: "Catch All"
    domain_regex: "."
"#;
    let test_app_builder = TestAppBuilder::new()
        .with_dns_resolver(mock_resolver.clone())
        .with_performance_config(PerformanceConfig {
            dns_worker_concurrency: worker_concurrency,
            rules_worker_concurrency: 1,
            queue_capacity: total_domains,
        })
        .with_outputs(vec![output.clone()])
        .with_test_domains_channel()
        .with_metrics()
        .with_disabled_periodic_health_check()
        .with_skipped_health_check()
        .with_rules(catch_all_rule);

    let mut test_app = test_app_builder.start().await.unwrap();
    test_app.wait_for_startup().await.unwrap();

    let start_time = Instant::now();

    for i in 0..total_domains {
        let domain = format!("domain-{}.com", i);
        mock_resolver.add_response(&domain, Ok(Default::default()));
        test_app.send_domain(&domain).await.unwrap();
    }

    // Close the channel to signal that no more domains are coming.
    test_app.close_domains_channel();

    completion_rx
        .await
        .expect("Failed to receive completion signal");
    let total_duration = start_time.elapsed();

    info!("--- Performance Test Results ---");
    info!("Total domains processed: {}", total_domains);
    info!("Total processing time: {:?}", total_duration);

    let expected_min_duration =
        (total_domains as u64 / worker_concurrency as u64) * resolution_delay.as_millis() as u64;

    assert!(
        total_duration.as_millis() as u64 > expected_min_duration,
        "Total duration ({:?}) should be greater than the ideal minimum ({:?})",
        total_duration,
        Duration::from_millis(expected_min_duration)
    );
}