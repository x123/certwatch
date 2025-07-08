//! Handles the dispatching of alerts to various notification channels.
//!
//! This module defines the core traits and structures for a decoupled notification
//! system. It uses a publisher/subscriber model, allowing the main application
//! to publish alerts without being aware of the specific notification
//! implementations that are listening for them.
pub mod logging_subscriber;