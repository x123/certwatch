//! Live integration test for the CertStream client.
//!
//! This test requires a local certstream server running at wss://127.0.0.1:8181
//! and is enabled with the `live-tests` feature flag.
//!
//! To run this test:
//! `cargo test --test live_certstream_client --features live-tests -- --nocapture`

#![cfg(feature = "live-tests")]

use anyhow::Result;
use std::time::Duration;
use tokio::sync::mpsc;

mod common;
use common::run_live_test;

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
