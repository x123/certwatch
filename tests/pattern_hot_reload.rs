//! Focused unit tests for the PatternWatcher.

use anyhow::Result;
use certwatch::core::PatternMatcher;
use certwatch::matching::PatternWatcher;
use std::io::Write;
use tempfile::NamedTempFile;
use tokio::sync::watch;

/// This test verifies the core `reload` functionality of the `PatternWatcher`
/// in a focused, deterministic way. It creates a pattern file, asserts the
/// initial state, modifies the file, calls `reload()` directly, and then
/// asserts that the watcher's internal state has been updated correctly.
/// This avoids the complexity and non-determinism of relying on real
/// filesystem watch events.
#[tokio::test]
async fn test_pattern_watcher_manual_reload() -> Result<()> {
    // 1. Setup: Create a temporary file for patterns.
    let mut temp_file = NamedTempFile::new()?;
    writeln!(temp_file, "initial.com")?;
    let pattern_path = temp_file.path().to_path_buf();

    // 2. Initialize the PatternWatcher.
    let (_shutdown_tx, shutdown_rx) = watch::channel(());
    let watcher = PatternWatcher::new(vec![pattern_path.clone()], shutdown_rx).await?;

    // 3. Initial State Verification: Ensure the first pattern is active.
    assert!(
        watcher.match_domain("test.initial.com").await.is_some(),
        "Initial pattern should match"
    );
    assert!(
        watcher.match_domain("test.updated.com").await.is_none(),
        "Updated pattern should not match yet"
    );

    // 4. Simulate File Change: Overwrite the pattern file with new content.
    // We use `std::fs` for a simple, synchronous write.
    let mut temp_file = temp_file.reopen()?;
    temp_file.set_len(0)?; // Clear the file
    writeln!(temp_file, "updated.com")?;
    temp_file.flush()?;

    // 5. Trigger Reload: Manually call the reload function.
    watcher.reload().await?;

    // 6. Final State Verification: Assert that the patterns have been updated.
    assert!(
        watcher.match_domain("test.initial.com").await.is_none(),
        "Initial pattern should be gone after reload"
    );
    assert!(
        watcher.match_domain("test.updated.com").await.is_some(),
        "Updated pattern should match after reload"
    );

    Ok(())
}

/// This test verifies that the `PatternWatcher` correctly handles a pattern
/// file being deleted. It initializes the watcher with a file, deletes the
/// file, calls `reload()`, and asserts that the patterns from the deleted
/// file are no longer active.
#[tokio::test]
async fn test_pattern_watcher_handles_file_deletion() -> Result<()> {
    // 1. Setup: Create two temporary files.
    let mut temp_file1 = NamedTempFile::new()?;
    writeln!(temp_file1, "pattern1.com")?;
    let path1 = temp_file1.path().to_path_buf();

    let mut temp_file2 = NamedTempFile::new()?;
    writeln!(temp_file2, "pattern2.com")?;
    let path2 = temp_file2.path().to_path_buf();

    // 2. Initialize the PatternWatcher with both files.
    let (_shutdown_tx, shutdown_rx) = watch::channel(());
    let watcher = PatternWatcher::new(vec![path1.clone(), path2.clone()], shutdown_rx).await?;

    // 3. Initial State Verification.
    assert!(watcher.match_domain("test.pattern1.com").await.is_some());
    assert!(watcher.match_domain("test.pattern2.com").await.is_some());

    // 4. Simulate File Deletion: Drop the temp file to delete it.
    drop(temp_file1);

    // 5. Trigger Reload.
    watcher.reload().await?;

    // 6. Final State Verification.
    assert!(
        watcher.match_domain("test.pattern1.com").await.is_none(),
        "Pattern from deleted file should not match"
    );
    assert!(
        watcher.match_domain("test.pattern2.com").await.is_some(),
        "Pattern from remaining file should still match"
    );

    Ok(())
}
