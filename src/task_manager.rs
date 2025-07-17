//! Manages the lifecycle of all spawned tasks in the application.
use futures::future::join_all;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

/// A centralized manager for all spawned tasks.
///
/// This struct is responsible for:
/// - Spawning tasks and keeping track of their `JoinHandle`s.
/// - Providing a graceful shutdown mechanism by awaiting all tasks.
#[derive(Clone, Debug)]
pub struct TaskManager {
    handles: Arc<Mutex<Vec<(&'static str, JoinHandle<()>)>>>,
    shutdown_rx: watch::Receiver<bool>,
}

impl TaskManager {
    /// Creates a new `TaskManager`.
    pub fn new(shutdown_rx: watch::Receiver<bool>) -> Self {
        Self {
            handles: Arc::new(Mutex::new(Vec::new())),
            shutdown_rx,
        }
    }

    /// Spawns a new task and adds its handle to the manager.
    pub fn spawn<F>(&self, name: &'static str, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        debug!(task_name = name, "Spawning task");
        let handle = tokio::spawn(future);
        self.handles.lock().unwrap().push((name, handle));
    }

    /// Returns a clone of the shutdown receiver.
    pub fn get_shutdown_rx(&self) -> watch::Receiver<bool> {
        self.shutdown_rx.clone()
    }

    /// Waits for all managed tasks to complete.
    pub async fn shutdown(self) {
        let handles = self.handles.lock().unwrap().drain(..).collect::<Vec<_>>();
        info!(
            "TaskManager shutting down. Waiting for {} tasks to complete...",
            handles.len()
        );

        let task_names: Vec<&'static str> = handles.iter().map(|(name, _)| *name).collect();
        debug!(tasks = ?task_names, "Awaiting all tasks.");

        let results = join_all(handles.into_iter().map(|(_, handle)| handle)).await;

        let mut panics = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            let task_name = task_names[i];
            match result {
                Ok(_) => {
                    debug!(task_name, "Task shut down gracefully.");
                }
                Err(e) => {
                    error!(task_name, "Task panicked during shutdown.");
                    panics.push((task_name, e));
                }
            }
        }

        if !panics.is_empty() {
            error!(
                "{} tasks panicked during shutdown: {:?}",
                panics.len(),
                panics
            );
        } else {
            info!("All tasks shut down gracefully.");
        }
    }
}