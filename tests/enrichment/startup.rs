use anyhow::Result;
use assert_cmd::prelude::*;
use std::process::Command;
use tempfile::NamedTempFile;
use std::io::Write;

fn certwatch_bin() -> Result<Command> {
    Ok(Command::cargo_bin("certwatch")?)
}

#[test]
fn test_startup_fails_if_enrichment_file_is_missing() -> Result<()> {
    let mut cmd = certwatch_bin()?;
    cmd.arg("--enrichment-asn-tsv-path")
        .arg("/tmp/this/file/does/not/exist.tsv")
        .arg("--test-mode");

    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("TSV database not found at"));

    Ok(())
}

#[test]
fn test_startup_fails_if_enrichment_file_is_malformed() -> Result<()> {
    let mut file = NamedTempFile::new()?;
    writeln!(file, "this is not valid tsv data")?;

    let mut cmd = certwatch_bin()?;
    cmd.arg("--enrichment-asn-tsv-path")
        .arg(file.path())
        .arg("--test-mode");

    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("CSV deserialize error"));

    Ok(())
}

#[test]
fn test_startup_succeeds_if_enrichment_is_not_configured() -> Result<()> {
    let mut cmd = certwatch_bin()?;
    cmd.arg("--test-mode");
    cmd.assert().success();
    Ok(())
}

#[test]
fn test_startup_succeeds_with_valid_enrichment_file() -> Result<()> {
    let mut file = NamedTempFile::new()?;
    writeln!(file, "1.1.1.0\t1.1.1.255\t13335\tUS\tCLOUDFLARENET")?;

    let mut cmd = certwatch_bin()?;
    cmd.arg("--enrichment-asn-tsv-path")
        .arg(file.path())
        .arg("--test-mode");

    cmd.assert().success();

    Ok(())
}