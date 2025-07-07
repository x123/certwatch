// CertWatch - Certificate Transparency Log Monitor
//
// A high-performance Rust application for monitoring certificate transparency
// logs and detecting suspicious domain registrations in real-time.

use anyhow::Result;
use certwatch::{
    cli::Cli,
    config::{Config, OutputFormat},
    dns::HickoryDnsResolver,
};
use clap::Parser;
use log::{error, info};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load configuration by layering sources: defaults, file, environment, and CLI args.
    let config = Config::load(&cli).unwrap_or_else(|err| {
        // Manually initialize logger for this specific error
        env_logger::init();
        error!("Failed to load configuration: {}", err);
        // Exit if configuration fails, as it's a critical step.
        std::process::exit(1);
    });

    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&config.log_level))
        .init();

    info!("CertWatch starting up...");

    // Log the loaded configuration settings for visibility
    log_config_settings(&config, &cli)?;

    // =========================================================================
    // Create Shutdown Channel
    // =========================================================================
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    // Spawn a task to listen for Ctrl-C
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to install Ctrl-C handler");
        info!("Shutdown signal received. Shutting down gracefully...");
        shutdown_tx.send(()).expect("Failed to send shutdown signal");
    });

    // Run the main application logic
    if let Err(e) = certwatch::app::run(config, shutdown_rx, None, None, None).await {
        error!("Application error: {}", e);
        std::process::exit(1);
    }

    info!("All tasks shut down. Exiting.");
    Ok(())
}

/// Logs the final, merged configuration settings to the console.
fn log_config_settings(config: &Config, cli: &Cli) -> Result<()> {
    info!("-------------------- Configuration --------------------");
    info!("Log Level: {}", config.log_level);
    info!("Log Metrics: {}", config.metrics.log_metrics);
    info!(
        "Log Aggregation Interval: {}s",
        config.metrics.log_aggregation_seconds
    );
    info!("Concurrency: {}", config.concurrency);
    info!(
        "Domain Queue Capacity: {}",
        config.performance.queue_capacity
    );
    info!("CertStream URL: {}", config.network.certstream_url);
    info!("Sample Rate: {}% (sample_rate:{})", (config.network.sample_rate * 100.0) as u64, config.network.sample_rate);
    let (_dns_resolver, nameservers) = HickoryDnsResolver::from_config(&config.dns)?;
    if let Some(resolver) = &config.dns.resolver {
        info!("DNS Resolver: {}", resolver);
    } else {
        let ns_str: Vec<String> = nameservers.iter().map(|s| s.to_string()).collect();
        info!("DNS Resolver: System Default ({})", ns_str.join(", "));
    }
    if let Some(timeout) = config.dns.timeout_ms {
        info!("DNS Timeout: {}ms", timeout);
    }
    info!(
        "DNS Standard Retries: {}",
        config.dns.retry_config.standard_retries
    );
    info!(
        "DNS Standard Backoff: {}ms",
        config.dns.retry_config.standard_initial_backoff_ms
    );
    info!(
        "DNS NXDOMAIN Retries: {}",
        config.dns.retry_config.nxdomain_retries
    );
    info!(
        "DNS NXDOMAIN Backoff: {}ms",
        config.dns.retry_config.nxdomain_initial_backoff_ms
    );
    if let Some(path) = &config.enrichment.asn_tsv_path {
        info!("ASN TSV Path: {}", path.display());
    } else {
        info!("ASN TSV Path: Not configured");
    }
    let output_format = if cli.json {
        OutputFormat::Json
    } else {
        config.output.format.clone()
    };
    info!("Output Format: {}", output_format);
    info!(
        "Slack Output: {}",
        if config.output.slack.is_some() {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    info!(
        "Deduplication Cache Size: {}",
        config.deduplication.cache_size
    );
    info!(
        "Deduplication Cache TTL: {}s",
        config.deduplication.cache_ttl_seconds
    );
    info!("-------------------------------------------------------");
    Ok(())
}
