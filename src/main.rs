// CertWatch - Certificate Transparency Log Monitor
//
// A high-performance Rust application for monitoring certificate transparency
// logs and detecting suspicious domain registrations in real-time.

use anyhow::Result;
use certwatch::config::{Config, OutputFormat};
use tokio::sync::watch;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration by layering sources: defaults, file, environment, and CLI args.
    let config = Config::load().unwrap_or_else(|err| {
        // We can't use the tracing `error!` macro here because the logger isn't initialized yet.
        eprintln!("[ERROR] Failed to load configuration: {}", err);
        std::process::exit(1);
    });

    // Initialize the logger with the final log level from the config.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(
                config.core.log_level.parse()
                    .unwrap(),
            ),
        )
        .init();

    info!("CertWatch starting up...");

    // Log the loaded configuration settings for visibility
    log_config_settings(&config);

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
    if config.test_mode {
        info!("Test mode enabled. Exiting after successful startup.");
        return Ok(());
    }

    // =========================================================================
    // Setup Notification Pipeline
    // =========================================================================
    let alert_tx = certwatch::services::setup_notification_pipeline(&config)?;

    // Build and run the main application
    let app_builder = certwatch::app::App::builder(config)
        .enrichment_provider_override(enrichment_provider);

    let app_builder = if let Some(tx) = alert_tx {
        app_builder.notification_tx(tx)
    } else {
        app_builder
    };

    let app = app_builder.build(shutdown_rx).await?;

    if let Err(e) = app.run().await {
        error!("Application error: {}", e);
        std::process::exit(1);
    }

    info!("All tasks shut down. Exiting.");
    Ok(())
}

/// Logs the final, merged configuration settings to the console.
fn log_config_settings(config: &Config) {
    info!("-------------------- Configuration --------------------");
    info!("Log Level: {}", config.core.log_level);
    info!("Log Metrics: {}", config.metrics.log_metrics);
    info!("Log Aggregation Interval: {}s", config.metrics.log_aggregation_seconds);
    info!("Concurrency: {}", config.core.concurrency);
    info!("Domain Queue Capacity: {}", config.performance.queue_capacity);
    info!("CertStream URL: {}", config.network.certstream_url);
    let sample_rate = config.network.sample_rate;
    info!("Sample Rate: {}% (sample_rate:{})", (sample_rate * 100.0) as u64, sample_rate);
    if let Some(resolver) = &config.dns.resolver {
        info!("DNS Resolver: {}", resolver);
    } else {
        info!("DNS Resolver: System Default");
    }
    info!("DNS Timeout: {}ms", config.dns.timeout_ms);
    info!("DNS Standard Retries: {}", config.dns.retry_config.retries.unwrap_or(3));
    info!("DNS Standard Backoff: {}ms", config.dns.retry_config.backoff_ms.unwrap_or(500));
    info!("DNS NXDOMAIN Retries: {}", config.dns.retry_config.nxdomain_retries.unwrap_or(1));
    info!("DNS NXDOMAIN Backoff: {}ms", config.dns.retry_config.nxdomain_backoff_ms.unwrap_or(0));
    if let Some(path) = &config.enrichment.asn_tsv_path {
        info!("ASN TSV Path: {}", path.display());
    } else {
        info!("ASN TSV Path: Not configured");
    }
    let output_format = config.output.format.clone().unwrap_or(OutputFormat::PlainText);
    info!("Output Format: {}", output_format);
    let slack_enabled = config.output.slack.as_ref().and_then(|s| s.enabled).unwrap_or(false);
    let slack_webhook_set = config.output.slack.as_ref().and_then(|s| s.webhook_url.as_ref()).is_some();
    info!("Slack Output: {}", if slack_enabled && slack_webhook_set { "Enabled" } else { "Disabled" });
    info!("Deduplication Cache Size: {}", config.deduplication.cache_size);
    info!("Deduplication Cache TTL: {}s", config.deduplication.cache_ttl_seconds);
    info!("-------------------------------------------------------");
}
