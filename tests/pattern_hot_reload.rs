//! Integration tests for pattern hot-reload functionality
//!
//! These tests verify that the PatternWatcher correctly detects file changes
//! and reloads patterns without interrupting service.

use anyhow::Result;
use certwatch::matching::{PatternWatcher, load_patterns_from_file};
use certwatch::core::PatternMatcher;
use tempfile::NamedTempFile;
use std::io::Write;
use tokio::sync::watch;

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
