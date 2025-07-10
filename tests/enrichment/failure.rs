//! Unit test for enrichment failure handling.

use std::{fs::File, io::Write, path::PathBuf};
use tempfile::tempdir;

/// Creates a temporary YAML rule file and returns its path.
fn create_rule_file(content: &str) -> PathBuf {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("rules.yml");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "{}", content).unwrap();
    // The tempdir is intentionally leaked here to prevent the file from being deleted.
    std::mem::forget(dir);
    file_path
}

// The test `test_process_domain_propagates_enrichment_error` is removed because
// the `process_domain` function is now fully asynchronous and no longer
// directly returns a result from the enrichment process. The error handling
// is now internal to the spawned tasks within the DnsResolutionManager.