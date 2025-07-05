//! CertWatch - A high-performance certificate transparency log monitor
//!
//! This library provides the core functionality for monitoring certificate
//! transparency logs and detecting suspicious domain registrations.

pub mod config;
pub mod core;
pub mod deduplication;
pub mod dns;
pub mod enrichment;
pub mod matching;
pub mod network;
pub mod outputs;

// Re-export core types for convenience
pub use core::*;
