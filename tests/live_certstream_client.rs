//! Live integration test for the CertStream client.
//!
//! This test requires a local certstream server running at wss://127.0.0.1:8181
//! and is enabled with the `live-tests` feature flag.
//!
//! To run this test:
//! `cargo test --test live_certstream_client --features live-tests -- --nocapture`

#![cfg(feature = "live-tests")]

use anyhow::Result;
use certwatch::{config::Config, network::CertStreamClient};
use futures::Future;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::error;

/// Helper to run a live test against a local certstream server.
async fn run_live_test<F, Fut>(duration: Duration, test_logic: F) -> Result<()>
where
    F: FnOnce(mpsc::Receiver<String>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (domains_tx, domains_rx) = mpsc::channel(100);
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    let mut config = Config::default();
    config.network.certstream_url = "ws://127.0.0.1:8181".to_string();
    config.network.allow_invalid_certs = true;

    let certstream_client = CertStreamClient::new(
        config.network.certstream_url.clone(),
        domains_tx,
        config.network.sample_rate,
        config.network.allow_invalid_certs,
    );

    let client_handle = tokio::spawn(async move {
        if let Err(e) = certstream_client.run(shutdown_rx).await {
            error!("CertStream client failed: {}", e);
        }
    });

    let logic_handle = tokio::spawn(test_logic(domains_rx));

    tokio::time::timeout(duration, logic_handle).await??;

    shutdown_tx.send(()).unwrap();
    client_handle.await?;

    Ok(())
}


#[tokio::test]
#[ignore] // This test requires a live server, so ignore by default
async fn test_live_connection_to_certstream() -> Result<()> {
    let test_duration = Duration::from_secs(10);

    let test_logic = |mut rx: mpsc::Receiver<String>| async move {
        log::info!("Waiting for the server to send a message...");

        // We'll just wait for any message to confirm the connection is established.
        let received_message = rx.recv().await;

        assert!(
            received_message.is_some(),
            "Test failed: Did not receive any message from the certstream server within {}s.",
            test_duration.as_secs()
        );

        if let Some(domain) = received_message {
            log::info!("Successfully received a domain from the live certstream server: '{}'. Test passed.", domain);
        }
    };

    run_live_test(test_duration, test_logic).await
}
