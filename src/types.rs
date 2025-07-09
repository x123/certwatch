//! Common type aliases used throughout the application.

use crate::core::Alert;
use tokio::sync::broadcast;

pub type AlertSender = broadcast::Sender<Alert>;