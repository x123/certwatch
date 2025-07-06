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
use std::time::Duration;
use tokio::sync::mpsc;
use chrono::Local;

mod common;
use common::run_live_test;

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
    // 1. Load patterns
    let patterns = load_patterns_from_file("tests/data/test-regex.txt").await?;
    let matcher = RegexMatcher::new(patterns)?;
    println!("[INFO] {} Loaded {} patterns.", Local::now().to_rfc3339(), matcher.patterns_count());

    // 2. Define the test logic using the harness
    let test_duration = Duration::from_secs(5);
    let test_logic = |mut rx: mpsc::Receiver<String>| async move {
        while let Some(domain) = rx.recv().await {
            if let Some(source_tag) = matcher.match_domain(&domain).await {
                println!(
                    "[MATCH] {} {}: '{}'",
                    Local::now().to_rfc3339(),
                    source_tag,
                    domain
                );
            }
        }
    };

    // 3. Run the test
    run_live_test(test_duration, test_logic).await
}
