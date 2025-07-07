use certwatch::cli::Cli;
use certwatch::config::{Config, OutputFormat};
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[test]
fn test_load_full_valid_config() {
    let toml_content = r#"
        log_level = "debug"
        concurrency = 4
        [metrics]
        log_metrics = true
        log_aggregation_seconds = 30
        [performance]
        queue_capacity = 50000
        [network]
        certstream_url = "ws://example.com/certstream"
        sample_rate = 0.5
        allow_invalid_certs = true
        [matching]
        pattern_files = ["/etc/certwatch/patterns.txt"]
        [dns]
        resolver = "1.1.1.1:53"
        timeout_ms = 2000
        standard_retries = 5
        standard_initial_backoff_ms = 100
        [dns.health]
        failure_threshold = 0.8
        window_seconds = 60
        recovery_check_domain = "cloudflare.com"
        recovery_check_interval_seconds = 5
        [enrichment]
        asn_tsv_path = "/var/db/ip-to-asn.tsv"
        [output]
        format = "Json"
        [output.slack]
        webhook_url = "https://hooks.slack.com/services/..."
        [deduplication]
        cache_size = 5000
        cache_ttl_seconds = 1800
    "#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let cli = Cli {
        config: Some(file.path().to_path_buf()),
        ..Default::default()
    };

    let config = Config::load(&cli).unwrap();

    assert_eq!(config.log_level, "debug");
    assert_eq!(config.concurrency, 4);
    assert!(config.metrics.log_metrics);
    assert_eq!(config.metrics.log_aggregation_seconds, 30);
    assert_eq!(config.performance.queue_capacity, 50000);
    assert_eq!(config.network.certstream_url, "ws://example.com/certstream");
    assert_eq!(config.network.sample_rate, 0.5);
    assert!(config.network.allow_invalid_certs);
    assert_eq!(
        config.matching.pattern_files,
        vec![PathBuf::from("/etc/certwatch/patterns.txt")]
    );
    assert_eq!(config.dns.resolver, Some("1.1.1.1:53".to_string()));
    assert_eq!(config.dns.timeout_ms, Some(2000));
    assert_eq!(config.dns.retry_config.standard_retries, 5);
    assert_eq!(config.dns.retry_config.standard_initial_backoff_ms, 100);
    assert_eq!(config.dns.retry_config.nxdomain_retries, 5); // This is not in the toml, so it should be the default value
    assert_eq!(
        config.dns.retry_config.nxdomain_initial_backoff_ms,
        10000
    ); // This is not in the toml, so it should be the default value
    assert_eq!(config.dns.health.failure_threshold, 0.8);
    assert_eq!(
        config.enrichment.asn_tsv_path,
        Some(PathBuf::from("/var/db/ip-to-asn.tsv"))
    );
    assert!(matches!(config.output.format, OutputFormat::Json));
    assert_eq!(
        config.output.slack.unwrap().webhook_url,
        "https://hooks.slack.com/services/..."
    );
    assert_eq!(config.deduplication.cache_size, 5000);
    assert_eq!(config.deduplication.cache_ttl_seconds, 1800);
}

#[test]
fn test_load_default_values() {
    let toml_content = r#""#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let cli = Cli {
        config: Some(file.path().to_path_buf()),
        ..Default::default()
    };

    let config = Config::load(&cli).unwrap();
    let default_config = Config::default();

    assert_eq!(config, default_config);
}
#[test]
fn test_invalid_value_type() {
    let toml_content = r#"
        concurrency = "four"
    "#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let cli = Cli {
        config: Some(file.path().to_path_buf()),
        ..Default::default()
    };

    let config = Config::load(&cli);
    assert!(config.is_err());
}
#[test]
fn test_missing_required_field() {
    let toml_content = r#"
        [output.slack]
        # webhook_url is missing
    "#;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();

    let cli = Cli {
        config: Some(file.path().to_path_buf()),
        ..Default::default()
    };

    let config = Config::load(&cli);
    assert!(config.is_err());
}