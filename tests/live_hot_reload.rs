//! Live integration test for the pattern hot-reload functionality.
//!
//! This test verifies that the `PatternWatcher` can dynamically update its
//! regex patterns from a file while processing a live CertStream feed.
//!
//! To run this test:
//! `cargo test --features live-tests -- --nocapture live_hot_reload`

#![cfg(feature = "live-tests")]

use anyhow::Result;
use certwatch::core::PatternMatcher;
use certwatch::matching::PatternWatcher;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use certwatch::network::CertStreamClient;
use std::time::Duration;
use tempfile::NamedTempFile;
use chrono::Local;
use tokio::sync::mpsc;
use tokio::time::timeout;

mod common;
use common::TEST_CERTSTREAM_URL;

#[tokio::test]
async fn live_hot_reload() -> Result<()> {
    // 1. Create a temporary file for patterns, initially empty
    let mut temp_file = NamedTempFile::new()?;
    writeln!(temp_file, "# Initially empty")?;
    let pattern_path = temp_file.path().to_path_buf();

    // 2. Create a channel to notify us when the reload is complete
    let (reload_tx, mut reload_rx) = mpsc::channel(1);

    // 3. Initialize the PatternWatcher with the notifier
    let pattern_files = vec![pattern_path.clone()];
    let watcher = PatternWatcher::with_notifier(pattern_files, Some(reload_tx)).await?;

    // 4. Setup communication channel and client
    let (tx, mut rx) = mpsc::channel(100);
    let client = CertStreamClient::new(TEST_CERTSTREAM_URL.to_string(), tx, 1.0, true);

    // 5. Spawn the client to run in the background
    tokio::spawn(async move {
        if let Err(e) = client.run().await {
            eprintln!("CertStreamClient run error: {}", e);
        }
    });

    // 6. Spawn a task to update the pattern file after a delay
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        println!("[TEST] {} Updating pattern file with new regex...", Local::now().to_rfc3339());
        let mut file = File::create(&pattern_path).unwrap();
        writeln!(file, r"\.com$").unwrap();
    });

    // 7. Wait for the reload to complete
    let reload_future = timeout(Duration::from_secs(10), reload_rx.recv());
    assert!(reload_future.await.is_ok(), "Timeout waiting for pattern reload");
    println!("[TEST] {} Reload notification received.", Local::now().to_rfc3339());

    // 8. Process messages and look for a match
    let test_duration = Duration::from_secs(15);
    let processing_future = async {
        while let Some(domains) = rx.recv().await {
            for domain in domains {
                if watcher.match_domain(&domain).await.is_some() {
                    // Match found, break the loop
                    return;
                }
            }
        }
    };

    // Run the processing future with a timeout
    let result = timeout(test_duration, processing_future).await;
    assert!(result.is_ok(), "Timeout waiting for a domain match after reload.");

    println!("[TEST] {} Domain match found after reload.", Local::now().to_rfc3339());

    Ok(())
}
