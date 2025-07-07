use metrics;
// Pattern matching engine for domain detection
//
// This module implements high-performance regex matching against thousands
// of patterns with hot-reload capability.

use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::{RegexSet, RegexSetBuilder};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use crate::core::PatternMatcher;
use crate::utils::heartbeat::run_heartbeat;
use tokio::sync::{mpsc, watch};
use tokio::time::Duration;

/// High-performance regex matcher using RegexSet for efficient multi-pattern matching
pub struct RegexMatcher {
    /// Compiled regex set for efficient matching against all patterns
    regex_set: RegexSet,
    /// Source tags corresponding to each pattern in the regex set
    tags: Vec<String>,
}

impl RegexMatcher {
    /// Creates a new RegexMatcher from a list of patterns and their associated tags
    ///
    /// # Arguments
    /// * `patterns` - Vector of (pattern, source_tag) tuples
    ///
    /// # Returns
    /// * `Ok(RegexMatcher)` if all patterns compile successfully
    /// * `Err` if any pattern fails to compile
    pub fn new(patterns: Vec<(String, String)>) -> Result<Self> {
        if patterns.is_empty() {
            return Ok(Self {
                regex_set: RegexSet::empty(),
                tags: Vec::new(),
            });
        }

        let (pattern_strings, tags): (Vec<String>, Vec<String>) = patterns.into_iter().unzip();
        let regex_set = RegexSetBuilder::new(&pattern_strings)
            .case_insensitive(true)
            .build()?;

        Ok(Self { regex_set, tags })
    }

    /// Returns the number of patterns loaded in the matcher.
    pub fn patterns_count(&self) -> usize {
        self.tags.len()
    }
}

#[async_trait]
impl PatternMatcher for RegexMatcher {
    /// Attempts to match a domain against loaded patterns
    ///
    /// # Arguments
    /// * `domain` - The domain name to check
    ///
    /// # Returns
    /// * `Some(source_tag)` if the domain matches a pattern
    /// * `None` if no patterns match
    async fn match_domain(&self, domain: &str) -> Option<String> {
        // Find the first matching pattern
        if let Some(match_index) = self.regex_set.matches(domain).into_iter().next() {
            // A match was found, get the corresponding tag
            if let Some(tag) = self.tags.get(match_index) {
                metrics::counter!("pattern_matches", "tag" => tag.clone()).increment(1);
                Some(tag.clone())
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Loads patterns from a file and derives the source tag from the filename
///
/// # Arguments
/// * `file_path` - Path to the pattern file
///
/// # Returns
/// * `Ok(Vec<(String, String)>)` containing (pattern, source_tag) pairs
/// * `Err` if the file cannot be read or the filename cannot be processed
pub async fn load_patterns_from_file<P: AsRef<Path>>(file_path: P) -> Result<Vec<(String, String)>> {
    let path = file_path.as_ref();
    let content = fs::read_to_string(path).await?;
    
    // Derive source tag from filename (without extension)
    let source_tag = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid filename: {:?}", path))?
        .to_string();

    let mut patterns = Vec::new();
    
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() == 2 {
            patterns.push((parts[0].to_string(), parts[1].to_string()));
        } else {
            // Fallback to using filename as tag if no tab is present
            patterns.push((line.to_string(), source_tag.clone()));
        }
    }

    Ok(patterns)
}

/// Pattern watcher that provides hot-reload capability for pattern files
#[derive(Clone)]
pub struct PatternWatcher {
    /// Current active matcher, atomically swappable
    current_matcher: Arc<ArcSwap<RegexMatcher>>,
    /// Watched pattern files
    pattern_files: Vec<PathBuf>,
}

impl PatternWatcher {
    /// Manually triggers a reload of the pattern files.
    pub async fn reload(&self) -> Result<()> {
        let new_patterns = Self::load_all_patterns(&self.pattern_files).await?;
        let new_matcher = RegexMatcher::new(new_patterns)?;
        self.current_matcher.store(Arc::new(new_matcher));
        Ok(())
    }
    /// Creates a new PatternWatcher and starts monitoring the specified files
    ///
    /// # Arguments
    /// * `pattern_files` - A list of file paths to watch for changes.
    ///
    /// # Returns
    /// * `Ok(PatternWatcher)` if initialization succeeds
    /// * `Err` if pattern loading or file watching setup fails
    pub async fn new(
        pattern_files: Vec<PathBuf>,
        mut shutdown_rx: watch::Receiver<()>,
    ) -> Result<Self> {
        Self::with_notifier(pattern_files, None, Some(&mut shutdown_rx)).await
    }

    /// Creates a new PatternWatcher with a reload notifier for testing
    pub async fn with_notifier(
        pattern_files: Vec<PathBuf>,
        reload_notifier: Option<mpsc::Sender<()>>,
        shutdown_rx: Option<&mut watch::Receiver<()>>,
    ) -> Result<Self> {
        // Load initial patterns from all files
        let initial_patterns = Self::load_all_patterns(&pattern_files).await?;
        let initial_matcher = RegexMatcher::new(initial_patterns)?;
        tracing::info!("Loaded initial {} patterns", initial_matcher.patterns_count());

        let current_matcher = Arc::new(ArcSwap::from_pointee(initial_matcher));

        let watcher = Self {
            current_matcher: current_matcher.clone(),
            pattern_files: pattern_files.clone(),
        };

        // Start file watching in background
        if let Some(shutdown_rx) = shutdown_rx {
            let hb_shutdown_rx = shutdown_rx.clone();
            tokio::spawn(async move {
                run_heartbeat("PatternWatcher", hb_shutdown_rx).await;
            });
            watcher
                .start_file_watcher(pattern_files, reload_notifier, shutdown_rx.clone())
                .await?;
        }

        Ok(watcher)
    }

    /// Loads patterns from all configured files
    async fn load_all_patterns(pattern_files: &[PathBuf]) -> Result<Vec<(String, String)>> {
        let mut all_patterns = Vec::new();
        for file_path in pattern_files {
            // Only attempt to load files that currently exist to avoid generating
            // noisy errors and potential event loops on deleted files.
            if tokio::fs::try_exists(file_path).await.unwrap_or(false) {
                match load_patterns_from_file(file_path).await {
                    Ok(mut patterns) => all_patterns.append(&mut patterns),
                    Err(e) => {
                        tracing::warn!("Failed to load patterns from {:?}: {}", file_path, e);
                        // Continue loading other files even if one fails
                    }
                }
            }
        }
        Ok(all_patterns)
    }

    /// Starts the file watcher in a background task
    async fn start_file_watcher(
        &self,
        pattern_files: Vec<PathBuf>,
        reload_notifier: Option<mpsc::Sender<()>>,
        mut shutdown_rx: watch::Receiver<()>,
    ) -> Result<()> {
        let current_matcher = self.current_matcher.clone();

        tokio::spawn(async move {
            tokio::select! {
                res = Self::run_file_watcher(current_matcher, pattern_files, reload_notifier) => {
                    if let Err(e) = res {
                        tracing::error!("File watcher error: {}", e);
                    }
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!("Pattern watcher received shutdown signal.");
                }
            }
        });

        Ok(())
    }

    /// Runs the file watcher loop
    /// Runs the file watcher loop with debouncing
    async fn run_file_watcher(
        current_matcher: Arc<ArcSwap<RegexMatcher>>,
        pattern_files: Vec<PathBuf>,
        reload_notifier: Option<mpsc::Sender<()>>,
    ) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(1); // Channel capacity can be small

        let watched_paths: Arc<HashSet<PathBuf>> = Arc::new(pattern_files.into_iter().collect());

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    // Ignore pure metadata events (like access/read) to prevent feedback
                    // loops where our own reload logic triggers another watch event.
                    // We are interested in creation, deletion, modification, and renames.
                    if !matches!(event.kind, EventKind::Access(_)) {
                        if let Err(e) = tx.blocking_send(()) {
                            // This can happen on shutdown, so a warning is sufficient.
                            tracing::warn!("Failed to send file event notification: {}", e);
                        }
                    }
                }
            },
            Config::default(),
        )?;

        // Watch all parent directories to capture renames, moves, and atomic writes.
        for path in watched_paths.iter() {
            if let Some(parent) = path.parent() {
                watcher.watch(parent, RecursiveMode::NonRecursive)?;
                tracing::info!("Watching for changes to pattern file: {:?}", path);
            }
        }

        const DEBOUNCE_DURATION: Duration = if cfg!(target_os = "macos") {
            Duration::from_millis(400) // Longer timeout for macOS FSEvents
        } else if cfg!(target_os = "windows") {
            Duration::from_millis(300)
        } else {
            Duration::from_millis(250) // Default for Linux and others
        };

        // Main event loop with debouncing logic
        loop {
            // Wait for the first event to start the debouncing window.
            if rx.recv().await.is_none() {
                break; // Exit if the channel is closed.
            }

            // Debounce: keep draining events until a quiet period is observed.
            loop {
                // If we don't receive another event within the debounce duration,
                // we can proceed with the reload.
                match tokio::time::timeout(DEBOUNCE_DURATION, rx.recv()).await {
                    Ok(Some(_)) => {
                        // Event received, continue draining.
                        continue;
                    }
                    Ok(None) => {
                        // Channel closed, exit the watcher loop.
                        return Ok(());
                    }
                    Err(_) => {
                        // Timeout elapsed, meaning a quiet period was observed.
                        // Break the drain loop to proceed with reloading.
                        break;
                    }
                }
            }

            tracing::info!("File activity detected. Reloading patterns after debounce.");

            let paths_to_load: Vec<PathBuf> = watched_paths.iter().cloned().collect();
            match Self::load_all_patterns(&paths_to_load).await {
                Ok(patterns) => match RegexMatcher::new(patterns) {
                    Ok(new_matcher) => {
                        let old_count = current_matcher.load().patterns_count();
                        let new_count = new_matcher.patterns_count();
                        let diff = new_count as isize - old_count as isize;
                        current_matcher.store(Arc::new(new_matcher));
                        tracing::info!(
                            "Successfully reloaded {} patterns ({:+})",
                            new_count,
                            diff
                        );
                        // Notify listeners that reload is complete
                        if let Some(ref notifier) = reload_notifier {
                            if notifier.send(()).await.is_err() {
                                tracing::warn!("Reload notifier channel closed");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to compile new patterns: {}", e);
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to load patterns for reload: {}", e);
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl PatternMatcher for PatternWatcher {
    /// Attempts to match a domain against loaded patterns
    ///
    /// # Arguments
    /// * `domain` - The domain name to check
    ///
    /// # Returns
    /// * `Some(source_tag)` if the domain matches a pattern
    /// * `None` if no patterns match
    async fn match_domain(&self, domain: &str) -> Option<String> {
        let matcher = self.current_matcher.load();
        matcher.match_domain(domain).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_regex_matcher_successful_match() {
        // Create patterns with different tags
        let patterns = vec![
            (r".*\.evil\.com$".to_string(), "malware".to_string()),
            (r".*phish.*".to_string(), "phishing".to_string()),
            (r".*typo.*".to_string(), "typosquatting".to_string()),
        ];

        let matcher = RegexMatcher::new(patterns).expect("Failed to create matcher");

        // Test successful matches
        let result = matcher.match_domain("subdomain.evil.com").await;
        assert_eq!(result, Some("malware".to_string()));

        let result = matcher.match_domain("phishing-site.com").await;
        assert_eq!(result, Some("phishing".to_string()));

        let result = matcher.match_domain("typo-amazon.com").await;
        assert_eq!(result, Some("typosquatting".to_string()));
    }

    #[tokio::test]
    async fn test_regex_matcher_no_match() {
        let patterns = vec![
            (r".*\.evil\.com$".to_string(), "malware".to_string()),
            (r".*phish.*".to_string(), "phishing".to_string()),
        ];

        let matcher = RegexMatcher::new(patterns).expect("Failed to create matcher");

        // Test domain that doesn't match any pattern
        let result = matcher.match_domain("legitimate-site.com").await;
        assert_eq!(result, None);

        let result = matcher.match_domain("google.com").await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_regex_matcher_correct_tag_mapping() {
        let patterns = vec![
            (r".*\.malicious\.com$".to_string(), "tag1".to_string()),
            (r".*\.suspicious\.com$".to_string(), "tag2".to_string()),
            (r".*\.dangerous\.com$".to_string(), "tag3".to_string()),
        ];

        let matcher = RegexMatcher::new(patterns).expect("Failed to create matcher");

        // Verify each pattern returns its correct tag
        let result = matcher.match_domain("test.malicious.com").await;
        assert_eq!(result, Some("tag1".to_string()));

        let result = matcher.match_domain("test.suspicious.com").await;
        assert_eq!(result, Some("tag2".to_string()));

        let result = matcher.match_domain("test.dangerous.com").await;
        assert_eq!(result, Some("tag3".to_string()));
    }

    #[tokio::test]
    async fn test_regex_matcher_empty_patterns() {
        let patterns = vec![];
        let matcher = RegexMatcher::new(patterns).expect("Failed to create matcher with empty patterns");

        // Should return None for any domain when no patterns are loaded
        let result = matcher.match_domain("any-domain.com").await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_regex_matcher_first_match_wins() {
        // Create patterns where a domain could match multiple patterns
        let patterns = vec![
            (r".*\.com$".to_string(), "generic".to_string()),
            (r".*evil.*".to_string(), "malware".to_string()),
        ];

        let matcher = RegexMatcher::new(patterns).expect("Failed to create matcher");

        // Domain matches both patterns, but should return the first match
        let result = matcher.match_domain("evil.com").await;
        // RegexSet returns matches in pattern order, so first pattern should win
        assert_eq!(result, Some("generic".to_string()));
    }

    #[test]
    fn test_regex_matcher_invalid_pattern() {
        // Test that invalid regex patterns return an error
        let patterns = vec![
            (r"[invalid regex(".to_string(), "invalid".to_string()),
        ];

        let result = RegexMatcher::new(patterns);
        assert!(result.is_err(), "Expected error for invalid regex pattern");
    }

    #[tokio::test]
    async fn test_load_patterns_from_file() {
        use tempfile::NamedTempFile;
        use std::io::Write;

        // Create a temporary file with test patterns
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let file_content = r#"# This is a comment
.*\.evil\.com$
.*phish.*

# Another comment
.*typo.*
"#;
        temp_file.write_all(file_content.as_bytes()).expect("Failed to write to temp file");
        
        // Load patterns from the file
        let patterns = load_patterns_from_file(temp_file.path()).await
            .expect("Failed to load patterns from file");

        // Verify the patterns were loaded correctly
        assert_eq!(patterns.len(), 3);
        
        // Check that source tag is derived from filename
        let expected_tag = temp_file.path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap()
            .to_string();

        assert!(patterns.contains(&(r".*\.evil\.com$".to_string(), expected_tag.clone())));
        assert!(patterns.contains(&(r".*phish.*".to_string(), expected_tag.clone())));
        assert!(patterns.contains(&(r".*typo.*".to_string(), expected_tag)));
    }

    #[tokio::test]
    async fn test_load_patterns_from_file_empty() {
        use tempfile::NamedTempFile;
        use std::io::Write;

        // Create a temporary file with only comments and empty lines
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let file_content = r#"# This is a comment

# Another comment

"#;
        temp_file.write_all(file_content.as_bytes()).expect("Failed to write to temp file");
        
        // Load patterns from the file
        let patterns = load_patterns_from_file(temp_file.path()).await
            .expect("Failed to load patterns from file");

        // Should return empty vector for file with no patterns
        assert_eq!(patterns.len(), 0);
    }

    #[tokio::test]
    async fn test_load_patterns_from_nonexistent_file() {
        let result = load_patterns_from_file("/nonexistent/file.txt").await;
        assert!(result.is_err(), "Expected error for nonexistent file");
    }
}
    #[tokio::test]
    async fn test_match_is_case_insensitive() {
        let patterns = vec![
            (r"example\.com".to_string(), "corp".to_string()),
        ];
        let matcher = RegexMatcher::new(patterns).unwrap();

        assert_eq!(
            matcher.match_domain("EXAMPLE.COM").await,
            Some("corp".to_string())
        );
        assert_eq!(
            matcher.match_domain("Example.Com").await,
            Some("corp".to_string())
        );
    }


    #[tokio::test]
    async fn test_empty_domain_input() {
        let patterns = vec![(r".+".to_string(), "any".to_string())];
        let matcher = RegexMatcher::new(patterns).unwrap();

        // Matching an empty domain should not match anything
        assert_eq!(matcher.match_domain("").await, None);
    }
