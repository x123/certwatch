//! Live integration test for the CertStream client.
//!
//! This test requires a local certstream server running at wss://127.0.0.1:8181
//! and is enabled with the `live-tests` feature flag.
//!
//! To run this test:
//! `cargo test --test live_certstream_client --features live-tests -- --nocapture`

#![cfg(feature = "live-tests")]

use certwatch::network::CertStreamClient;
use tokio::sync::mpsc;

mod common;
use common::TEST_CERTSTREAM_URL;

#[tokio::test]
#[ignore] // This test requires a live server, so ignore by default
async fn test_live_connection_to_certstream() {
    // Initialize logging to see output from the client
    let _ = env_logger::builder().is_test(true).try_init();

    // Create a channel to receive parsed domains
    let (tx, mut rx) = mpsc::channel::<Vec<String>>(100);

    // Create the client
    let client = CertStreamClient::new(TEST_CERTSTREAM_URL.to_string(), tx);

    // Run the client in a separate task
    let client_handle = tokio::spawn(async move {
        if let Err(e) = client.run().await {
            log::error!("Client run failed: {}", e);
        }
    });

    log::info!("Waiting for the server to acknowledge the connection...");

    // The server should send a welcome message or similar.
    // We'll just wait for any message to confirm the connection is established and the server is responsive.
    let received_message = tokio::time::timeout(
        std::time::Duration::from_secs(10), // Allow 10s for the server to send a message
        rx.recv()
    ).await;

    assert!(
        received_message.is_ok(),
        "Test failed: Did not receive any message from the certstream server within 10s. Is the server running and sending messages?"
    );

    log::info!("Successfully received a message from the live certstream server. Test passed.");

    // Clean up the client task
    client_handle.abort();
}
