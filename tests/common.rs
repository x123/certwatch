//! Common utilities, constants, and fixtures for integration tests.

#![allow(dead_code)] // Allow unused constants

use anyhow::Result;
use certwatch::network::CertStreamClient;
use std::future::Future;
use chrono::Local;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

pub const TEST_CERTSTREAM_URL: &str = "wss://127.0.0.1:8181/domains-only";

/// A test harness for running live integration tests against the CertStream feed.
///
/// This function encapsulates the common boilerplate for:
/// 1. Initializing the logger.
/// 2. Setting up an `mpsc` channel for receiving domains.
/// 3. Creating and spawning the `CertStreamClient`.
/// 4. Running a provided test logic future (`test_logic`) that receives the domains.
/// 5. Applying a timeout to the test.
///
/// # Arguments
///
/// * `duration` - The `Duration` for which the test should run.
/// * `test_logic` - An async block or function that takes a `mpsc::Receiver<String>`
///   and performs the specific assertions for the test.
pub async fn run_live_test<F, Fut>(duration: Duration, test_logic: F) -> Result<()>
where
    F: FnOnce(mpsc::Receiver<String>) -> Fut,
    Fut: Future<Output = ()>,
{
    // Initialize the logger to see output from the client
    let _ = env_logger::try_init();
    println!("[INFO] {} Starting live test for {} seconds...", Local::now().to_rfc3339(), duration.as_secs());

    // Setup communication channel
    let (tx, rx) = mpsc::channel(100);

    // Connect to CertStream
    let client = CertStreamClient::new(TEST_CERTSTREAM_URL.to_string(), tx, 1.0, true); // 1.0 sample rate for tests, allow invalid certs
    println!("CertStreamClient initialized.");

    // Spawn the client to run in the background
    let (_shutdown_tx, shutdown_rx) = watch::channel(());
    tokio::spawn(async move {
        if let Err(e) = client.run(shutdown_rx).await {
            eprintln!("CertStreamClient run error: {}", e);
        }
    });

    // Run the provided test logic with a timeout
    if let Err(e) = timeout(duration, test_logic(rx)).await {
        println!("[INFO] {} Test finished after {} seconds: {}", Local::now().to_rfc3339(), duration.as_secs(), e);
    } else {
        println!("[INFO] {} Test finished after {} seconds.", Local::now().to_rfc3339(), duration.as_secs());
    }

    Ok(())
}
