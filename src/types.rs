//! Common type aliases used throughout the application.

use crate::core::Alert;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub type DomainReceiver = Arc<Mutex<mpsc::Receiver<String>>>;
pub type AlertSender = mpsc::Sender<Alert>;