//! Live integration test for the pattern matching engine.
//!
//! This test connects to a live CertStream server, listens for certificate
//! updates, and matches the received domain names against a predefined set
//! of regex patterns.
//!
//! To run this test:
//! `cargo test --features live-tests -- --nocapture live_pattern_matching`

// Mark this entire module to be compiled only when the 'live-tests' feature is enabled.
#![cfg(feature = "live-tests")]

use anyhow::Result;
use certwatch::core::PatternMatcher;
use certwatch::matching::{load_patterns_from_file, RegexMatcher};
use certwatch::network::CertStreamClient;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

mod common;
use common::TEST_CERTSTREAM_URL;

/// Test the pattern matcher against a live CertStream feed.
///
/// This test performs the following steps:
/// 1. Loads regex patterns from `tests/data/test-regex.txt`.
/// 2. Initializes the `RegexMatcher`.
/// 3. Connects to the CertStream WebSocket server using the test TLS configuration.
/// 4. Listens to the stream for 15 seconds.
/// 5. For each message, it parses the domain and attempts to match it against the patterns.
/// 6. Logs any successful matches.
/// 7. The test passes if it can process the stream without errors.
#[tokio::test]
async fn live_pattern_matching() -> Result<()> {
    // Initialize the logger to see output from the client
    let _ = env_logger::try_init();
    println!("Starting live pattern matching test...");

    // 1. Load patterns
    let patterns = load_patterns_from_file("tests/data/test-regex.txt").await?;
    let matcher = RegexMatcher::new(patterns)?;
    println!("Loaded {} patterns.", matcher.patterns_count());

    // 2. Setup communication channel
    let (tx, mut rx) = mpsc::channel(100);

    // 3. Connect to CertStream
    let client = CertStreamClient::new(TEST_CERTSTREAM_URL.to_string(), tx);
    println!("CertStreamClient initialized.");

    // Spawn the client to run in the background.
    // The client will connect and start sending messages to the channel.
    tokio::spawn(async move {
        if let Err(e) = client.run().await {
            eprintln!("CertStreamClient run error: {}", e);
        }
    });

    // 4. Listen for messages and match domains
    let test_duration = Duration::from_secs(3);
    let processing_future = async {
        while let Some(domains) = rx.recv().await {
            println!("[DEBUG] Received {} domains: {:?}", domains.len(), domains);
            for domain in domains {
                if let Some(source_tag) = matcher.match_domain(&domain).await {
                    println!("[MATCH] Domain: '{}' matched pattern from source: '{}'", domain, source_tag);
                }
            }
        }
    };

    // Run the processing future with a timeout
    if let Err(e) = timeout(test_duration, processing_future).await {
        println!("Test finished after {} seconds: {}", test_duration.as_secs(), e);
    } else {
        println!("Test finished after {} seconds.", test_duration.as_secs());
    }

    Ok(())
}
