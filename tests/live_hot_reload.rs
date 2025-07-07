//! Live integration test for the pattern hot-reload functionality.
//!
//! This test verifies that the `PatternWatcher` can dynamically update its
//! regex patterns from a file while processing a live CertStream feed.
//!
//! To run this test:
//! `cargo test --features live-tests -- --nocapture live_hot_reload`

use anyhow::Result;
use certwatch::core::PatternMatcher;
use certwatch::matching::PatternWatcher;
use std::io::Write;
use certwatch::network::CertStreamClient;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

mod helpers;
use helpers::fs_watch::{platform_aware_write, PlatformTimeouts};
use helpers::mock_ws::MockWebSocket;

#[tokio::test]
async fn test_hot_reload_with_mock_ws() -> Result<()> {
    // 1. Create a temporary file for patterns, initially empty
    let mut temp_file = NamedTempFile::new()?;
    writeln!(temp_file, "# Initially empty")?;
    let pattern_path = temp_file.path().to_path_buf();

    // 2. Create a channel to notify us when the reload is complete
    let (reload_tx, mut reload_rx) = mpsc::channel(1);

    // 3. Initialize the PatternWatcher with the notifier
    let pattern_files = vec![pattern_path.clone()];
    let (_shutdown_tx, mut shutdown_rx) = watch::channel(());
    let watcher =
        PatternWatcher::with_notifier(pattern_files, Some(reload_tx), Some(&mut shutdown_rx))
            .await?;

    // 4. Setup mock WebSocket, communication channel, and client
    let mock_ws = MockWebSocket::new();
    let (tx, mut rx) = mpsc::channel(100);
    let client = CertStreamClient::new("ws://mock.url".to_string(), tx, 1.0, false);

    // 5. Spawn the client to run in the background with the mock connection
    let client_task = tokio::spawn({
        let mock_ws_clone = mock_ws.clone();
        async move {
            client
                .run_with_connection(Box::new(mock_ws_clone))
                .await
        }
    });

    // 6. Spawn a task to update the pattern file
    let update_task = tokio::spawn(async move {
        let timeouts = PlatformTimeouts::for_current_platform();
        platform_aware_write(&pattern_path, r"\.com$", &timeouts)
            .await
            .unwrap();
    });
    update_task.await?; // Ensure the write is complete

    // 7. Wait for the reload to complete
    let reload_future = timeout(Duration::from_secs(5), reload_rx.recv());
    assert!(reload_future.await.is_ok(), "Timeout waiting for pattern reload");

    // 8. Now that patterns are loaded, send a matching domain through the mock WebSocket
    mock_ws.add_domain_message("matching-domain.com");

    // 9. Process messages and look for a match
    let test_duration = Duration::from_secs(5);
    let processing_future = async {
        while let Some(domain) = rx.recv().await {
            if watcher.match_domain(&domain).await.is_some() {
                assert_eq!(domain, "matching-domain.com");
                return; // Match found, test passes
            }
        }
        panic!("Stream ended without finding a match");
    };

    // Run the processing future with a timeout
    timeout(test_duration, processing_future)
        .await
        .expect("Timeout waiting for a domain match after reload.");

    // 10. Clean up
    mock_ws.close();
    client_task.abort();
    Ok(())
}
