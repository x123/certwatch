//! Command-Line Interface (CLI) argument parsing.
//!
//! This module defines the command-line arguments for the application using the
//! `clap` crate. These arguments are parsed at startup and then merged with
//! the configuration from the `certwatch.toml` file and environment variables.

use clap::Parser;
use std::path::PathBuf;

/// A high-performance, real-time Certificate Transparency Log monitor.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the TOML configuration file.
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Deduplication window duration in seconds.
    #[arg(long)]
    pub dedup_window: Option<u64>,

    /// Sampling rate for the certstream (0.0 to 1.0).
    #[arg(long)]
    pub sample_rate: Option<f64>,

    /// Timeout for DNS resolution in milliseconds.
    #[arg(long)]
    pub dns_timeout_ms: Option<u64>,

    /// IP address of the DNS resolver to use.
    #[arg(long)]
    pub dns_resolver: Option<String>,

    /// Enable the real-time metrics display.
    #[arg(long)]
    pub live_metrics: bool,
}