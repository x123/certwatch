use tracing::trace;
use tokio::time::{interval, Duration};

/// Spawns a background task that logs a "heartbeat" message periodically.
///
/// This is a debugging utility designed to help identify "zombie" tasks that
/// fail to terminate during graceful shutdown. Each key background component
/// should spawn a heartbeat task with a unique name. If a heartbeat message

/// continues to be logged after the shutdown signal has been sent, it indicates
/// that the corresponding task is not respecting the shutdown signal.
pub async fn run_heartbeat(
    task_name: &'static str,
    mut shutdown_rx: tokio::sync::watch::Receiver<()>,
) {
    let mut timer = interval(Duration::from_secs(3));
    trace!(task_name, "Heartbeat started.");
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                trace!(task_name, "Heartbeat received shutdown. Exiting.");
                break;
            }
            _ = timer.tick() => {
                trace!(task_name, "Heartbeat is alive.");
            }
        }
    }
}