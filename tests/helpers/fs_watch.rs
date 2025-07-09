#![allow(dead_code)]
//! Deterministic file system watching test helpers.
//!
//! This module provides utilities for testing file system watching behavior
//! in a fast and deterministic way by replacing `sleep`-based waits with
//! explicit signaling using `tokio::sync::oneshot` channels.

use anyhow::Result;
use std::path::PathBuf;
use tokio::fs;
use tokio::sync::oneshot;

/// A signal that a file change has been completed.
/// The receiver can be awaited by the test to ensure the file operation
/// has finished before proceeding with assertions.
pub type FileChangeSignal = oneshot::Receiver<()>;

/// A handle to a temporary test file that allows for signaled writes.
pub struct TestFile {
    path: PathBuf,
    // Keep the temp file alive until TestFile is dropped.
    _temp_file: tempfile::NamedTempFile,
}

impl TestFile {
    /// Creates a new test file with initial content.
    pub fn new(initial_content: &str) -> Result<Self> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();
        std::fs::write(&path, initial_content)?;
        Ok(Self {
            path,
            _temp_file: temp_file,
        })
    }

    /// Returns the path to the test file.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Overwrites the file with new content and sends a signal on completion.
    pub async fn write(&self, content: &str) -> FileChangeSignal {
        let (tx, rx) = oneshot::channel();
        tokio::fs::write(&self.path, content)
            .await
            .expect("Failed to write to test file");
        // The write is complete, send the signal.
        let _ = tx.send(());
        rx
    }

    /// Appends content to the file and sends a signal on completion.
    pub async fn append(&self, content: &str) -> FileChangeSignal {
        use tokio::io::AsyncWriteExt;
        let (tx, rx) = oneshot::channel();
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&self.path)
            .await
            .expect("Failed to open test file for appending");
        file.write_all(content.as_bytes())
            .await
            .expect("Failed to append to test file");
        file.flush().await.expect("Failed to flush test file");
        // The append is complete, send the signal.
        let _ = tx.send(());
        rx
    }

    /// Deletes the file and sends a signal on completion.
    pub async fn delete(self) -> FileChangeSignal {
        let (tx, rx) = oneshot::channel();
        // Dropping the temp file handle deletes the file.
        drop(self._temp_file);
        // The deletion is complete, send the signal.
        let _ = tx.send(());
        rx
    }
}

/// Waits for a reload notification from a channel with a timeout.
/// This is a generic helper that can be used in any test that needs to
/// wait for a message on an mpsc channel.
pub async fn wait_for_notification<T>(
    receiver: &mut tokio::sync::mpsc::Receiver<T>,
    timeout_duration: std::time::Duration,
) -> Result<T, &'static str> {
    match tokio::time::timeout(timeout_duration, receiver.recv()).await {
        Ok(Some(notification)) => Ok(notification),
        Ok(None) => Err("Notification channel was closed prematurely"),
        Err(_) => Err("Timeout waiting for notification"),
    }
}
