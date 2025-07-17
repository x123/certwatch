#[path = "../helpers/mod.rs"]
mod helpers;

use certwatch::core::DnsInfo;
use helpers::{app::TestAppBuilder, mock_dns::MockDnsResolver};
use std::{net::Ipv4Addr, sync::Arc, time::Duration};
use tokio::sync::oneshot;

#[tokio::test]
async fn test_dns_worker_scheduling_delay_metric() {
    let (completion_tx, completion_rx) = oneshot::channel();

    // 1. Configure the TestAppBuilder with a low number of DNS workers
    let mock_dns = MockDnsResolver::new(Arc::new(certwatch::internal_metrics::Metrics::new_for_test()))
        .with_delay(Duration::from_millis(100))
        .with_completion_signal(completion_tx);

    mock_dns.add_response(
        "one.com",
        Ok(DnsInfo {
            a_records: vec![Ipv4Addr::new(1, 1, 1, 1).into()],
            ..Default::default()
        }),
    );

    let test_app = TestAppBuilder::new()
        .with_config_modifier(|config| {
            config.performance.dns_worker_concurrency = 1;
        })
        .with_dns_resolver(Arc::new(mock_dns))
        .with_test_domains_channel()
        .with_metrics()
        .with_skipped_health_check()
        .start()
        .await
        .unwrap();

    // 2. Inject domains into the app
    test_app.send_domain("one.com").await.unwrap();

    // 3. Wait for the DNS resolution to complete
    completion_rx
        .await
        .expect("Failed to receive completion signal");

    // 4. Fetch metrics and assert
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{}/metrics", test_app.metrics_addr()))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let count_line = response
        .lines()
        .find(|line| line.starts_with("dns_worker_scheduling_delay_seconds_count"))
        .unwrap();
    let count: u64 = count_line.split_whitespace().last().unwrap().parse().unwrap();
    assert_eq!(count, 1, "The count for the metric should be 1");

    let sum_line = response
        .lines()
        .find(|line| line.starts_with("dns_worker_scheduling_delay_seconds_sum"))
        .unwrap();
    let sum: f64 = sum_line.split_whitespace().last().unwrap().parse().unwrap();
    assert!(sum > 0.0, "The sum should be greater than 0");

    test_app.shutdown(Duration::from_secs(1)).await.unwrap();
}