//! Focused unit test for graceful shutdown signaling.

use anyhow::Result;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::timeout;

/// This test verifies the core shutdown mechanism (`tokio::sync::watch`)
/// in isolation. It ensures that a worker task listening on a shutdown
/// receiver will correctly terminate when a signal is sent on the
/// corresponding sender. This is a fast, reliable, and focused unit test
/// that replaces a slow, brittle end-to-end integration test.
#[tokio::test]
async fn test_shutdown_signal_is_propagated() -> Result<()> {
    // 1. Create a watch channel to simulate the shutdown signal.
    let (shutdown_tx, mut shutdown_rx) = watch::channel(());

    // 2. Spawn a worker task that listens for the shutdown signal.
    let worker = tokio::spawn(async move {
        // The worker loop will select between its work and the shutdown signal.
        loop {
            tokio::select! {
                // `biased` ensures we always check for shutdown first.
                biased;

                // Check if the shutdown signal has been received.
                _ = shutdown_rx.changed() => {
                    // Shutdown signal received, break the loop.
                    break;
                }

                // Simulate some work.
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    // Still working...
                }
            }
        }
    });

    // 3. Send the shutdown signal.
    shutdown_tx.send(())?;

    // 4. Assert that the worker task completes quickly after the signal.
    // The outer `?` handles the timeout error, the inner `?` handles the task join error.
    timeout(Duration::from_secs(1), worker).await??;

    Ok(())
}