//! Integration tests for DNS failure handling.

use anyhow::Result;
use certwatch::{
    core::DnsInfo,
    dns::DnsError,
};
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::Duration,
};
use tempfile::NamedTempFile;

mod helpers;
use helpers::{
    app::TestAppBuilder,
    mock_dns::MockDnsResolver,
    mock_output::FailableMockOutput,
    mock_ws::MockWebSocket,
    test_metrics::TestMetrics,
};

#[tokio::test]
async fn test_dns_scenarios() -> Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();
    let metrics = TestMetrics::new();
    metrics::set_global_recorder(metrics.clone()).unwrap();

    // --- Scenario 1: Timeout Failure ---
    {
        let mock_dns = MockDnsResolver::new();
        mock_dns.add_response(
            "timeout.com",
            Err(DnsError::Resolution("timeout".to_string())),
        );
        let mut pattern_file = NamedTempFile::new()?;
        std::io::Write::write_all(&mut pattern_file, b"timeout.com")?;
        let mut app_builder = TestAppBuilder::new()
            .with_dns_resolver(Arc::new(mock_dns))
            .with_enrichment_provider(Arc::new(helpers::fake_enrichment::FakeEnrichmentProvider::new()))
            .with_pattern_files(vec![pattern_file.path().to_path_buf()]);
        let asn_file = NamedTempFile::new()?;
        app_builder.config.enrichment.asn_tsv_path = Some(asn_file.path().to_path_buf());
        let mock_ws = MockWebSocket::new();
        let app = app_builder.with_websocket(Box::new(mock_ws.clone())).build().await?;
        mock_ws.add_domain_message("timeout.com");
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(metrics.get_counter("cert_processing_failures"), 1, "Timeout test: failures should be 1");
        assert_eq!(metrics.get_counter("cert_processing_successes"), 0, "Timeout test: successes should be 0");
        mock_ws.close();
        app.shutdown(Duration::from_secs(5)).await?;
    }

    // --- Scenario 2: NXDOMAIN Success ---
    {
        let mock_dns = MockDnsResolver::new();
        mock_dns.add_response(
            "nxdomain.com",
            Err(DnsError::Resolution("NXDOMAIN".to_string())),
        );
        let mock_output = Arc::new(FailableMockOutput::new());
        let mut pattern_file = NamedTempFile::new()?;
        std::io::Write::write_all(&mut pattern_file, b"nxdomain.com")?;
        let mut app_builder = TestAppBuilder::new()
            .with_dns_resolver(Arc::new(mock_dns))
            .with_enrichment_provider(Arc::new(helpers::fake_enrichment::FakeEnrichmentProvider::new()))
            .with_outputs(vec![mock_output.clone()])
            .with_pattern_files(vec![pattern_file.path().to_path_buf()]);
        let asn_file = NamedTempFile::new()?;
        app_builder.config.enrichment.asn_tsv_path = Some(asn_file.path().to_path_buf());
        let mock_ws = MockWebSocket::new();
        let app = app_builder.with_websocket(Box::new(mock_ws.clone())).build().await?;
        mock_ws.add_domain_message("nxdomain.com");
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(metrics.get_counter("cert_processing_failures"), 1, "NXDOMAIN test: failures should still be 1");
        assert_eq!(metrics.get_counter("cert_processing_successes"), 1, "NXDOMAIN test: successes should be 1");
        assert_eq!(mock_output.alerts.lock().unwrap().len(), 1, "An alert should have been sent for the NXDOMAIN");
        mock_ws.close();
        app.shutdown(Duration::from_secs(5)).await?;
    }

    // --- Scenario 3: Standard Success ---
    {
        let mock_dns = MockDnsResolver::new();
        mock_dns.add_response(
            "success.com",
            Ok(DnsInfo {
                a_records: vec![IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))],
                ..Default::default()
            }),
        );
        let mock_output = Arc::new(FailableMockOutput::new());
        let mut pattern_file = NamedTempFile::new()?;
        std::io::Write::write_all(&mut pattern_file, b"success.com")?;
        let mut app_builder = TestAppBuilder::new()
            .with_dns_resolver(Arc::new(mock_dns))
            .with_enrichment_provider(Arc::new(helpers::fake_enrichment::FakeEnrichmentProvider::new()))
            .with_outputs(vec![mock_output.clone()])
            .with_pattern_files(vec![pattern_file.path().to_path_buf()]);
        let asn_file = NamedTempFile::new()?;
        app_builder.config.enrichment.asn_tsv_path = Some(asn_file.path().to_path_buf());
        let mock_ws = MockWebSocket::new();
        let app = app_builder.with_websocket(Box::new(mock_ws.clone())).build().await?;
        mock_ws.add_domain_message("success.com");
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(metrics.get_counter("cert_processing_failures"), 1, "Success test: failures should still be 1");
        assert_eq!(metrics.get_counter("cert_processing_successes"), 2, "Success test: successes should be 2");
        assert_eq!(mock_output.alerts.lock().unwrap().len(), 1, "An alert should have been sent for the successful domain");
        mock_ws.close();
        app.shutdown(Duration::from_secs(5)).await?;
    }

    Ok(())
}