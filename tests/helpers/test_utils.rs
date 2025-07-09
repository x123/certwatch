#![allow(dead_code)]
use std::{fs::File, io::Write, path::PathBuf};

pub fn create_rule_file(content: &str) -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("rules.yml");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "{}", content).unwrap();
    // The tempdir is intentionally leaked here to prevent the file from being deleted
    // before the test runner can access it. This is a common pattern in tests.
    std::mem::forget(dir);
    file_path
}