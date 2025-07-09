use anyhow::Result;
use assert_cmd::prelude::*;
use std::process::Command;
use tempfile::NamedTempFile;
use std::io::Write;

fn certwatch_bin() -> Result<Command> {
    Ok(Command::cargo_bin("certwatch")?)
}

/// Helper to create a temporary config file with enrichment settings.
fn create_config_file(asn_tsv_path: &str) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new()?;
    writeln!(
        file,
        r#"
[enrichment]
asn_tsv_path = "{}"
"#,
        asn_tsv_path
    )?;
    Ok(file)
}

#[test]
fn test_startup_fails_if_enrichment_file_is_missing() -> Result<()> {
    let config_file = create_config_file("/tmp/this/file/does/not/exist.tsv")?;
    let mut cmd = certwatch_bin()?;
    cmd.arg("--config-file")
        .arg(config_file.path())
        .arg("--test-mode");

    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("TSV database not found at"));

    Ok(())
}

#[test]
fn test_startup_fails_if_enrichment_file_is_malformed() -> Result<()> {
    let mut tsv_file = NamedTempFile::new()?;
    writeln!(tsv_file, "this is not valid tsv data")?;

    let config_file = create_config_file(tsv_file.path().to_str().unwrap())?;

    let mut cmd = certwatch_bin()?;
    cmd.arg("--config-file")
        .arg(config_file.path())
        .arg("--test-mode");

    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("CSV deserialize error"));

    Ok(())
}

#[test]
fn test_startup_succeeds_if_enrichment_is_not_configured() -> Result<()> {
    let mut config_file = NamedTempFile::new()?;
    writeln!(config_file, "")?; // Empty config

    let mut cmd = certwatch_bin()?;
    cmd.arg("--config-file")
        .arg(config_file.path())
        .arg("--test-mode");
    cmd.assert().success();
    Ok(())
}

#[test]
fn test_startup_succeeds_with_valid_enrichment_file() -> Result<()> {
    let mut tsv_file = NamedTempFile::new()?;
    writeln!(tsv_file, "1.1.1.0\t1.1.1.255\t13335\tUS\tCLOUDFLARENET")?;

    let config_file = create_config_file(tsv_file.path().to_str().unwrap())?;

    let mut cmd = certwatch_bin()?;
    cmd.arg("--config-file")
        .arg(config_file.path())
        .arg("--test-mode");

    cmd.assert().success();

    Ok(())
}