use std::io::{Read, Write};
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

#[tokio::test]
async fn test_json_output_is_clean_and_logs_go_to_stderr() {
    // Arrange: Create a temporary config file that is valid for startup
    let mut temp_config = NamedTempFile::new().expect("Failed to create temp config file");
    let config_content = r#"
# Use a minimal valid config for the test run.
# By omitting the 'enrichment' section, we trigger the NoOpEnrichmentProvider
# and avoid the need for a real TSV file.
[output]
format = "Json"
"#;
    temp_config.write_all(config_content.as_bytes()).expect("Failed to write to temp config file");

    // Act
    let mut child = Command::new(env!("CARGO_BIN_EXE_certwatch"))
        .arg("--config-file")
        .arg(temp_config.path())
        .arg("--test-mode")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn certwatch process");

    let mut stdout = String::new();
    let mut stderr = String::new();

    child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
    child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();

    let status = child.wait().expect("Failed to wait for certwatch process");

    // Assert
    assert!(status.success(), "Certwatch process should exit successfully. Stderr: {}", stderr);

    // Check stdout for valid JSON (or empty in test mode, which is expected)
    assert!(stdout.is_empty(), "stdout should be empty in test mode, but was: {}", stdout);

    // Check stderr for log messages
    assert!(stderr.contains("CertWatch starting up..."), "stderr should contain startup logs");
    assert!(stderr.contains("Output Format: JSON"), "stderr should log the correct output format");
    assert!(stderr.contains("Test mode enabled."), "stderr should contain test mode log");
}