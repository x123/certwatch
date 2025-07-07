//! Integration tests for graceful shutdown.

use anyhow::Result;
use std::time::Duration;

// Import the test harness
mod helpers;
use helpers::app::TestAppBuilder;

/// A high-level integration test that runs the entire application with mock inputs
/// and asserts that it terminates within a strict time limit after a shutdown signal is sent.
/// This acts as a primary, unambiguous signal for a deadlock.
#[tokio::test]
async fn test_app_shuts_down_within_timeout() -> Result<()> {
    // Initialize the logger to see output from the application
    let _ = env_logger::builder().is_test(true).try_init();

    // Build and run the application in the background
    let app = TestAppBuilder::new().build().await?;

    // Give the app a moment to start up its services
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Shut down the app and assert it finishes within the timeout
    app.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}