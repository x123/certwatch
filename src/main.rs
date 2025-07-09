// CertWatch - Certificate Transparency Log Monitor
//
// A high-performance Rust application for monitoring certificate transparency
// logs and detecting suspicious domain registrations in real-time.

use anyhow::Result;
use certwatch::{
    cli::Cli,
    config::{Config, OutputFormat},
    core::Alert,
};
use clap::Parser;
use tokio::sync::{broadcast, watch};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging ASAP. We'll use a default log level for now.
    // The final log level will be determined by the config.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(
            "info".parse().unwrap(),
        ))
        .init();

    info!("CertWatch starting up...");

    // Load configuration by layering sources: defaults, file, environment, and CLI args.
    let config = Config::load(&cli).unwrap_or_else(|err| {
        error!("Failed to load configuration: {}", err);
        std::process::exit(1);
    });

    // Re-initialize the logger with the final log level from the config.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(config.log_level.parse().unwrap()),
        )
        .try_init();


    // Log the loaded configuration settings for visibility
    log_config_settings(&config, &cli);

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

    // =========================================================================
    // Pre-flight Checks
    // =========================================================================
    info!("Performing Enrichment data check...");
    let enrichment_provider =
        certwatch::enrichment::health::startup_check(&config)
            .await
            .map_err(|e| {
                error!("Enrichment data check failed: {}", e);
                e
            })?;
    info!("Enrichment data check successful.");

    // In test mode, exit immediately after successful startup checks.
    if cli.test_mode {
        info!("Test mode enabled. Exiting after successful startup.");
        return Ok(());
    }

    // =========================================================================
    // Setup Notification Pipeline
    // =========================================================================
    let alert_tx = {
        let mut enabled = false;
        if let Some(slack_config) = &config.output.slack {
            if slack_config.enabled {
                if slack_config.webhook_url.is_empty() {
                    tracing::warn!("Slack notifications are enabled, but no webhook URL was provided. Slack notifications will be disabled.");
                } else {
                    enabled = true;
                }
            }
        }

        if enabled {
            let (tx, _rx) = broadcast::channel::<Alert>(config.performance.queue_capacity);
            info!("Slack notification pipeline enabled.");

            // Spawn the Slack notifier.
            let slack_config = config.output.slack.as_ref().unwrap().clone();
            let slack_formatter =
                Box::new(certwatch::formatting::SlackTextFormatter);
            let slack_client =
                std::sync::Arc::new(certwatch::notification::slack::SlackClient::new(
                    slack_config.webhook_url.clone(),
                    slack_formatter,
                ));
            let slack_notifier = certwatch::notification::manager::NotificationManager::new(
                slack_config,
                tx.subscribe(),
                slack_client,
            );
            tokio::spawn(slack_notifier.run());
            Some(tx)
        } else {
            None
        }
    };

    // Run the main application logic
    let (domains_tx, _) = broadcast::channel(config.performance.queue_capacity);

    if let Err(e) = certwatch::app::run(
        config,
        shutdown_rx,
        domains_tx,
        None,
        None,
        None,
        Some(enrichment_provider),
        alert_tx,
    )
    .await
    {
        error!("Application error: {}", e);
        std::process::exit(1);
    }

    info!("All tasks shut down. Exiting.");
    Ok(())
}

/// Logs the final, merged configuration settings to the console.
fn log_config_settings(config: &Config, cli: &Cli) {
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
    if let Some(resolver) = &config.dns.resolver {
        info!("DNS Resolver: {}", resolver);
    } else {
        info!("DNS Resolver: System Default");
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
        if config
            .output
            .slack
            .as_ref()
            .is_some_and(|s| s.enabled && !s.webhook_url.is_empty())
        {
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
}
