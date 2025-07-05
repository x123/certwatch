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
use gag::BufferRedirect;
use std::io::Read;
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

    // 2. Initialize the PatternWatcher
    let mut pattern_files = HashMap::new();
    pattern_files.insert(pattern_path.clone(), "live-hot-reload-test".to_string());
    let watcher = PatternWatcher::new(pattern_files).await?;

    // 3. Setup communication channel and client
    let (tx, mut rx) = mpsc::channel(100);
    let client = CertStreamClient::new(TEST_CERTSTREAM_URL.to_string(), tx);

    // 4. Capture stdout to check for reload messages
    let mut stdout_buf = BufferRedirect::stdout()?;

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

    // 7. Process messages and look for a match
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
    let _ = timeout(test_duration, processing_future).await;

    // 8. Verify that the reload happened by checking the captured output
    let mut output = String::new();
    stdout_buf.read_to_string(&mut output)?;
    drop(stdout_buf); // Release stdout

    println!("{}", output); // Print captured output for debugging

    assert!(
        output.contains("[RELOAD-COMPLETED]"),
        "Test failed: Did not find '[RELOAD-COMPLETED]' in stdout."
    );

    Ok(())
}
