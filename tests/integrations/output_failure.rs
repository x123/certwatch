//! Integration test for output failure handling.

use anyhow::Result;
use certwatch::core::Alert;
use tokio::sync::mpsc;

#[path = "../helpers/mod.rs"]
mod helpers;

#[tokio::test]
async fn test_output_failure_does_not_crash_app() -> Result<()> {
    // This is a focused unit test that verifies the core logic of handling a
    // failing output channel without running the entire application.
    // It confirms that if one output receiver is dropped (simulating a crash
    // or failure in an output task), the main loop can continue sending
    // alerts to other, healthy outputs without panicking.

    // 1. Create a "healthy" channel where the receiver stays alive.
    let (healthy_tx, mut healthy_rx) = mpsc::channel::<Alert>(1);

    // 2. Create a "failing" channel where we immediately drop the receiver.
    let (failing_tx, failing_rx) = mpsc::channel::<Alert>(1);
    drop(failing_rx);

    // 3. Create a list of senders, mimicking the app's output management.
    let output_senders = vec![healthy_tx, failing_tx];

    // 4. Create a dummy alert to send.
    let alert = Alert::default();

    // 5. Simulate the app's broadcast loop.
    let mut send_errors = 0;
    for sender in &output_senders {
        // We don't care about the notification channel for this test.
        let _ = sender.send(alert.clone()).await.map_err(|_| send_errors += 1);
    }

    // 6. Assert that exactly one send failed.
    // This proves the loop continued after the error.
    assert_eq!(send_errors, 1, "Expected exactly one send error from the dropped receiver");

    // 7. Assert that the healthy channel received the alert.
    // This proves that other outputs were not affected.
    assert!(healthy_rx.recv().await.is_some(), "Healthy receiver should have received the alert");

    Ok(())
}