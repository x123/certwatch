use certwatch::config::{Cli, Config, OutputFormat};
use clap::Parser;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

/// A helper function to run a test with a temporary config file.
fn with_config_file<F>(toml_content: &str, test_fn: F)
where
    F: FnOnce(PathBuf),
{
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", toml_content).unwrap();
    let path = file.path().to_path_buf();
    test_fn(path);
}

#[test]
fn test_load_full_valid_config() {
    let toml_content = r#"
        [core]
        log_level = "debug"
        [performance]
        dns_worker_concurrency = 4
        rules_worker_concurrency = 4
        queue_capacity = 50000
        [network]
        certstream_url = "ws://example.com/certstream"
        sample_rate = 0.5
        allow_invalid_certs = true
        [rules]
        rule_files = ["/etc/certwatch/rules.yml"]
        [dns]
        resolver = "1.1.1.1:53"
        timeout_ms = 2000
        retries = 5
        backoff_ms = 100
        nxdomain_retries = 10
        nxdomain_backoff_ms = 10000
        [dns.health]
        interval_seconds = 15
        check_domain = "test.com"
        [enrichment]
        asn_tsv_path = "/var/db/ip-to-asn.tsv"
        [output]
        format = "Json"
        [output.slack]
        enabled = true
        webhook_url = "https://hooks.slack.com/services/..."
        batch_size = 50
        batch_timeout_seconds = 30
        [deduplication]
        cache_size = 5000
        cache_ttl_seconds = 1800
    "#;

    with_config_file(toml_content, |path| {
        let cli = Cli::try_parse_from(&["certwatch", "--config-file", path.to_str().unwrap()]).unwrap();
        let config = Config::load_from_cli(cli).unwrap();

        assert_eq!(config.core.log_level, "debug".to_string());
        assert_eq!(config.performance.dns_worker_concurrency, 4);
        assert_eq!(config.performance.rules_worker_concurrency, 4);
        assert_eq!(config.performance.queue_capacity, 50000);
        assert_eq!(
            config.network.certstream_url,
            "ws://example.com/certstream".to_string()
        );
        assert_eq!(config.network.sample_rate, 0.5);
        assert_eq!(config.network.allow_invalid_certs, true);
        assert_eq!(
            config.rules.rule_files,
            Some(vec![PathBuf::from("/etc/certwatch/rules.yml")])
        );
        assert_eq!(config.dns.resolver, Some("1.1.1.1:53".to_string())); // resolver is still optional
        assert_eq!(config.dns.timeout_ms, 2000);
        assert_eq!(config.dns.retry_config.retries, Some(5));
        assert_eq!(config.dns.retry_config.backoff_ms, Some(100));
        assert_eq!(config.dns.retry_config.nxdomain_retries, Some(10));
        assert_eq!(
            config.dns.retry_config.nxdomain_backoff_ms,
            Some(10000)
        );
        assert_eq!(config.dns.health.interval_seconds, 15);
        assert_eq!(config.dns.health.check_domain, "test.com".to_string());
        assert_eq!(
            config.enrichment.asn_tsv_path,
            Some(PathBuf::from("/var/db/ip-to-asn.tsv"))
        );
        assert!(matches!(config.output.format, Some(OutputFormat::Json)));

        let slack_config = config.output.slack.as_ref().unwrap();
        assert_eq!(slack_config.enabled, Some(true));
        assert_eq!(
            slack_config.webhook_url,
            Some("https://hooks.slack.com/services/...".to_string())
        );
        assert_eq!(slack_config.batch_size, Some(50));
        assert_eq!(slack_config.batch_timeout_seconds, Some(30));
        assert_eq!(config.deduplication.cache_size, 5000);
        assert_eq!(config.deduplication.cache_ttl_seconds, 1800);
    });
}

#[test]
fn test_load_partial_config_uses_defaults() {
    let toml_content = r#"
        [core]
        log_level = "warn"
        [dns]
        resolver = "8.8.8.8:53"
    "#;

    with_config_file(toml_content, |path| {
        let cli = Cli::try_parse_from(&["certwatch", "--config-file", path.to_str().unwrap()]).unwrap();
        let config = Config::load_from_cli(cli).unwrap();

        // Values from file
        assert_eq!(config.core.log_level, "warn".to_string());
        assert_eq!(config.dns.resolver, Some("8.8.8.8:53".to_string()));

        // Values from Default
        assert_eq!(config.performance.dns_worker_concurrency, 256);
        assert_eq!(config.performance.queue_capacity, 100_000);
        assert_eq!(
            config.network.certstream_url,
            "wss://certstream.calidog.io".to_string()
        );
        assert_eq!(config.dns.retry_config.retries, Some(3));
        assert!(config.output.slack.is_none());
    });
}

#[test]
fn test_invalid_value_type() {
    let toml_content = r#"
        [performance]
        dns_worker_concurrency = "four" # Invalid type
    "#;

    with_config_file(toml_content, |path| {
        let cli = Cli::try_parse_from(&["certwatch", "--config-file", path.to_str().unwrap()]).unwrap();
        let config_result = Config::load_from_cli(cli);
        assert!(config_result.is_err());
        let error_string = config_result.unwrap_err().to_string();
        assert!(error_string.contains("invalid type: found string \"four\", expected usize for key \"default.performance.dns_worker_concurrency\""));
    });
}

#[test]
fn test_non_existent_config_file() {
    let non_existent_path = PathBuf::from("/path/to/non/existent/config.toml");
    let cli = Cli::try_parse_from(&["certwatch", "--config-file", non_existent_path.to_str().unwrap()]).unwrap();
    let config_result = Config::load_from_cli(cli);
    assert!(config_result.is_err());
    let error_string = config_result.unwrap_err().to_string();
    assert!(error_string.contains("Config file not found at specified path"));
}