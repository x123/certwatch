//! Integration tests for pattern hot-reload functionality
//!
//! These tests verify that the PatternWatcher correctly detects file changes
//! and reloads patterns without interrupting service.

use anyhow::Result;
use certwatch::matching::{PatternWatcher, load_patterns_from_file};
use certwatch::core::PatternMatcher;
use std::io::Write;
use tempfile::NamedTempFile;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;
use serial_test::serial;

mod helpers;
use helpers::fs_watch::{
    PlatformTimeouts, platform_aware_write, platform_aware_append, 
    wait_for_watcher_ready, wait_for_reload_notification, create_isolated_test_env
};

#[tokio::test]
async fn test_pattern_watcher_multiple_files() -> Result<()> {
    // Initialize logging for the test
    let _ = env_logger::builder().is_test(true).try_init();

    // Create two temporary pattern files
    let mut temp_file1 = NamedTempFile::new()?;
    let mut temp_file2 = NamedTempFile::new()?;
    let temp_path1 = temp_file1.path().to_path_buf();
    let temp_path2 = temp_file2.path().to_path_buf();
    
    // Write initial patterns to both files
    temp_file1.write_all(b".*\\.malware\\.com$\n")?;
    temp_file1.flush()?;
    
    temp_file2.write_all(b".*\\.phishing\\.com$\n")?;
    temp_file2.flush()?;

    // Create a list of pattern files
    let pattern_files = vec![temp_path1, temp_path2];

    // Create PatternWatcher
    let (_shutdown_tx, shutdown_rx) = watch::channel(());
    let watcher = PatternWatcher::new(pattern_files, shutdown_rx).await?;

    // Test that patterns from both files work
    let result = watcher.match_domain("test.malware.com").await;
    assert!(result.is_some(), "Malware pattern should match");

    let result = watcher.match_domain("test.phishing.com").await;
    assert!(result.is_some(), "Phishing pattern should match");

    Ok(())
}

#[tokio::test]
async fn test_load_patterns_from_file_integration() -> Result<()> {
    // Create a temporary file with various pattern formats
    let mut temp_file = NamedTempFile::new()?;
    let file_content = r#"# Phishing patterns for testing
# This file contains regex patterns for detecting phishing domains

# Common phishing patterns
.*paypal.*
.*amazon.*secure.*
.*bank.*login.*

# Empty line above should be ignored

# Typosquatting patterns  
.*gooogle.*
.*microsooft.*
"#;
    temp_file.write_all(file_content.as_bytes())?;
    temp_file.flush()?;
    
    // Load patterns from the file
    let patterns = load_patterns_from_file(temp_file.path()).await?;

    // Verify correct number of patterns loaded (should skip comments and empty lines)
    assert_eq!(patterns.len(), 5);
    
    // Verify source tag is derived from filename
    let expected_tag = temp_file.path()
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap()
        .to_string();

    // Check that all patterns have the correct tag
    for (_, tag) in &patterns {
        assert_eq!(tag, &expected_tag);
    }

    // Verify specific patterns are present
    let pattern_strings: Vec<String> = patterns.into_iter().map(|(p, _)| p).collect();
    assert!(pattern_strings.contains(&".*paypal.*".to_string()));
    assert!(pattern_strings.contains(&".*amazon.*secure.*".to_string()));
    assert!(pattern_strings.contains(&".*gooogle.*".to_string()));

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_debounced_hot_reload() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    // Get platform-specific timeouts
    let timeouts = PlatformTimeouts::for_current_platform();

    // --- Setup ---
    // Create an isolated temporary directory to ensure clean test environment
    let temp_dir = create_isolated_test_env().await?;
    let temp_path = temp_dir.path().join("patterns.txt");

    // Initial pattern - use platform-aware write
    platform_aware_write(&temp_path, "initial.com\n", &timeouts).await?;

    let (reload_tx, mut reload_rx) = mpsc::channel(10);
    let (_shutdown_tx, mut shutdown_rx) = watch::channel(());

    let watcher = PatternWatcher::with_notifier(
        vec![temp_path.clone()],
        Some(reload_tx),
        Some(&mut shutdown_rx),
    )
    .await?;

    // Wait for the watcher to be fully initialized
    wait_for_watcher_ready(&timeouts).await;

    // --- Initial State Verification ---
    assert!(
        watcher.match_domain("initial.com").await.is_some(),
        "Initial pattern should match"
    );
    assert!(
        watcher.match_domain("final.com").await.is_none(),
        "Final pattern should not match yet"
    );

    // --- Simulate Rapid File Changes ---
    // 1. Overwrite with a new pattern
    platform_aware_write(&temp_path, "interim.com\n", &timeouts).await?;
    
    // Small delay between operations to ensure they're seen as separate events
    sleep(timeouts.inter_operation_delay).await;

    // 2. Append another pattern 
    platform_aware_append(&temp_path, "final.com\n", &timeouts).await?;

    // --- Verification ---
    // Wait for the debounced reload to occur with platform-appropriate timeout
    match wait_for_reload_notification(&mut reload_rx, &timeouts).await {
        Ok(_) => log::info!("Reload detected."),
        Err(msg) => panic!("Watcher did not reload after file modification: {}", msg),
    }

    // Ensure only one reload occurred despite multiple writes.
    assert!(
        reload_rx.try_recv().is_err(),
        "Should only be one reload notification"
    );

    // Give the watcher a moment to fully process the reload
    sleep(timeouts.fs_event_propagation).await;

    // Verify the final state of the patterns.
    assert!(
        watcher.match_domain("initial.com").await.is_none(),
        "Initial pattern should be gone"
    );
    assert!(
        watcher.match_domain("interim.com").await.is_some(),
        "Interim pattern should match"
    );
    assert!(
        watcher.match_domain("final.com").await.is_some(),
        "Final pattern should match"
    );

    Ok(())
}

#[tokio::test]
#[serial]
async fn test_hot_reload_on_delete() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    let timeouts = PlatformTimeouts::for_current_platform();

    // --- Setup ---
    let temp_dir = create_isolated_test_env().await?;
    let path1 = temp_dir.path().join("patterns1.txt");
    let path2 = temp_dir.path().join("patterns2.txt");

    platform_aware_write(&path1, "pattern1.com\n", &timeouts).await?;
    platform_aware_write(&path2, "pattern2.com\n", &timeouts).await?;

    let (reload_tx, mut reload_rx) = mpsc::channel(10);
    let (_shutdown_tx, mut shutdown_rx) = watch::channel(());

    let watcher = PatternWatcher::with_notifier(
        vec![path1.clone(), path2.clone()],
        Some(reload_tx),
        Some(&mut shutdown_rx),
    )
    .await?;

    wait_for_watcher_ready(&timeouts).await;

    // --- Initial State Verification ---
    assert!(watcher.match_domain("site-pattern1.com").await.is_some(), "Pattern 1 should match initially");
    assert!(watcher.match_domain("site-pattern2.com").await.is_some(), "Pattern 2 should match initially");

    // --- Simulate File Deletion ---
    tokio::fs::remove_file(&path1).await?;

    // --- Verification ---
    match wait_for_reload_notification(&mut reload_rx, &timeouts).await {
        Ok(_) => log::info!("Reload detected after file deletion."),
        Err(msg) => panic!("Watcher did not reload after file deletion: {}", msg),
    }

    sleep(timeouts.fs_event_propagation).await;

    // Verify the final state of the patterns.
    assert!(watcher.match_domain("site-pattern1.com").await.is_none(), "Pattern 1 should be gone after deletion");
    assert!(watcher.match_domain("site-pattern2.com").await.is_some(), "Pattern 2 should still match");

    Ok(())
}
