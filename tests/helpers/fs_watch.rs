#![allow(dead_code)]
//! Cross-platform file system watching test helpers
//!
//! This module provides utilities for testing file system watching behavior
//! across different platforms (Linux, macOS, Windows) with appropriate timeouts
//! and platform-specific adjustments.

use std::path::Path;
use std::time::Duration;
use tokio::fs;

/// Platform-specific timeouts for file system operations
pub struct PlatformTimeouts {
    /// Time to wait for file system events to propagate
    pub fs_event_propagation: Duration,
    /// Debounce timeout - should be longer than the actual debounce in the code
    pub debounce_timeout: Duration,
    /// Total timeout for receiving reload notifications
    pub reload_notification_timeout: Duration,
    /// Time to sleep between file operations to ensure they're detected as separate events
    pub inter_operation_delay: Duration,
}

impl PlatformTimeouts {
    /// Get platform-appropriate timeouts based on the current OS
    pub fn for_current_platform() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self {
                fs_event_propagation: Duration::from_millis(100),
                debounce_timeout: Duration::from_millis(500), // Longer than the 250ms debounce
                reload_notification_timeout: Duration::from_secs(3),
                inter_operation_delay: Duration::from_millis(100),
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            Self {
                fs_event_propagation: Duration::from_millis(50),
                debounce_timeout: Duration::from_millis(300),
                reload_notification_timeout: Duration::from_secs(2),
                inter_operation_delay: Duration::from_millis(50),
            }
        }
        
        #[cfg(target_os = "windows")]
        {
            Self {
                fs_event_propagation: Duration::from_millis(200),
                debounce_timeout: Duration::from_millis(400),
                reload_notification_timeout: Duration::from_secs(3),
                inter_operation_delay: Duration::from_millis(150),
            }
        }
        
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            // Conservative defaults for unknown platforms
            Self {
                fs_event_propagation: Duration::from_millis(200),
                debounce_timeout: Duration::from_millis(500),
                reload_notification_timeout: Duration::from_secs(4),
                inter_operation_delay: Duration::from_millis(200),
            }
        }
    }
}

/// Performs a platform-aware file write operation
/// 
/// This function ensures that file writes are performed in a way that's most likely
/// to trigger file system events on the current platform.
pub async fn platform_aware_write<P: AsRef<Path>>(
    path: P, 
    content: &str,
    timeouts: &PlatformTimeouts
) -> Result<(), anyhow::Error> {
    let path = path.as_ref();
    
    #[cfg(target_os = "macos")]
    {
        // On macOS, we need to be more careful about atomic writes
        // Some editors and tools use atomic write operations that may not trigger
        // the expected events. We'll use a simple write and then sync.
        fs::write(path, content).await?;
        
        // Force a sync to ensure the file system events are triggered
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.sync_all();
        }
    }
    
    #[cfg(not(target_os = "macos"))]
    {
        fs::write(path, content).await?;
    }
    
    // Give the file system time to propagate the event
    tokio::time::sleep(timeouts.fs_event_propagation).await;
    
    Ok(())
}

/// Performs a platform-aware file append operation
pub async fn platform_aware_append<P: AsRef<Path>>(
    path: P,
    content: &str,
    timeouts: &PlatformTimeouts
) -> Result<(), anyhow::Error> {
    let path = path.as_ref();
    
    use tokio::io::AsyncWriteExt;
    
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    
    file.write_all(content.as_bytes()).await?;
    file.flush().await?;
    
    #[cfg(target_os = "macos")]
    {
        // Force sync on macOS
        file.sync_all().await?;
    }
    
    drop(file);
    
    // Give the file system time to propagate the event
    tokio::time::sleep(timeouts.fs_event_propagation).await;
    
    Ok(())
}

/// Waits for a file watcher to be ready to receive events
/// 
/// File watchers may need some time to set up their monitoring infrastructure,
/// especially on macOS where the underlying FSEvents API may have setup delays.
pub async fn wait_for_watcher_ready(_timeouts: &PlatformTimeouts) {
    // Platform-specific delays to ensure the watcher is fully set up
    #[cfg(target_os = "macos")]
    {
        // macOS FSEvents can take longer to initialize
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    
    #[cfg(target_os = "linux")]
    {
        // inotify is usually faster to set up
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // Conservative default
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Robust helper for testing file system reload notifications
/// 
/// This function handles platform-specific timing and retry logic to make
/// file system watching tests more reliable across different platforms.
pub async fn wait_for_reload_notification<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    timeouts: &PlatformTimeouts,
) -> Result<T, &'static str> {
    let recv_future = receiver.recv();
    
    // Pin the future to the stack
    tokio::pin!(recv_future);

    // Poll the future with a timeout
    match tokio::time::timeout(timeouts.reload_notification_timeout, &mut recv_future).await {
        Ok(Some(notification)) => Ok(notification),
        Ok(None) => Err("Notification channel was closed"),
        Err(_) => {
            // If we are using a paused clock, advance time to simulate the timeout
            tokio::time::advance(timeouts.reload_notification_timeout).await;
            Err("Timeout waiting for reload notification")
        }
    }
}

/// Creates a test environment that's isolated between test runs
/// 
/// This helps prevent interference between different hot-reload tests
/// by ensuring each test uses a completely separate temporary directory.
pub async fn create_isolated_test_env() -> Result<tempfile::TempDir, anyhow::Error> {
    let temp_dir = tempfile::TempDir::new()?;
    
    // Ensure the directory exists and is writable
    let path = temp_dir.path();
    if !path.exists() {
        fs::create_dir_all(path).await?;
    }
    
    Ok(temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_platform_timeouts() {
        let timeouts = PlatformTimeouts::for_current_platform();
        
        // Verify that timeouts are reasonable
        assert!(timeouts.fs_event_propagation < Duration::from_secs(1));
        assert!(timeouts.debounce_timeout > Duration::from_millis(100));
        assert!(timeouts.reload_notification_timeout > timeouts.debounce_timeout);
    }
    
    #[tokio::test]
    async fn test_isolated_test_env() {
        let env = create_isolated_test_env().await.unwrap();
        let path = env.path();
        
        assert!(path.exists());
        assert!(path.is_dir());
        
        // Test that we can write to the directory
        let test_file = path.join("test.txt");
        fs::write(&test_file, "test content").await.unwrap();
        assert!(test_file.exists());
    }
}
