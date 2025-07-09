use certwatch::{
    config::RulesConfig,
    core::{Alert, DnsInfo},
    rules::{EnrichmentLevel, Rule, RuleMatcher},
};
use std::{fs::File, io::Write, path::PathBuf};
use tempfile::tempdir;

fn create_rule_file(content: &str) -> PathBuf {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("rules.yml");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "{}", content).unwrap();
    // The tempdir is intentionally leaked here to prevent the file from being deleted
    // before the test runner can access it. This is a common pattern in tests.
    std::mem::forget(dir);
    file_path
}

fn create_test_alert(domain: &str) -> Alert {
    Alert {
        domain: domain.to_string(),
        dns: DnsInfo::default(),
        enrichment: Vec::new(),
        ..Default::default()
    }
}

#[test]
fn test_rule_grouping_and_compilation() {
    let rule_content = r#"
- name: "Combined Rule"
  all:
    - domain_regex: "^test1\\.com$"
- name: "Single Rule"
  all:
    - domain_regex: "^single\\.com$"
- name: "Combined Rule"
  all:
    - domain_regex: "^test2\\.net$"
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };

    let rules = Rule::load_from_files(&config).unwrap();
    assert_eq!(rules.len(), 2, "Rules with the same name should be grouped");

    let combined_rule = rules
        .iter()
        .find(|r| r.name == "Combined Rule")
        .expect("Combined Rule not found");
    let single_rule = rules
        .iter()
        .find(|r| r.name == "Single Rule")
        .expect("Single Rule not found");

    // Check that the combined regex matches both original patterns
    let regex = combined_rule.domain_regex.as_ref().unwrap();
    assert!(regex.is_match("test1.com"));
    assert!(regex.is_match("test2.net"));
    assert!(!regex.is_match("test3.org"));

    // Check the single rule
    let regex = single_rule.domain_regex.as_ref().unwrap();
    assert!(regex.is_match("single.com"));
    assert!(!regex.is_match("notsingle.com"));
}

#[test]
fn test_rule_classification() {
    // TODO: Re-enable this test once ASN/IP conditions are re-implemented
    // in the new rule structure. For now, all rules are Stage 1.
    let rule_content = r#"
- name: "Stage 1 Rule"
  all:
    - domain_regex: "stage1"
- name: "Stage 2 Rule"
  all:
    - asns: [123]
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };
    let matcher = RuleMatcher::new(&config).unwrap();

    assert_eq!(matcher.stage_1_rules.len(), 1);
    assert_eq!(matcher.stage_1_rules[0].name, "Stage 1 Rule");
    assert_eq!(matcher.stage_2_rules.len(), 1);
    assert_eq!(matcher.stage_2_rules[0].name, "Stage 2 Rule");
}

#[test]
fn test_combined_domain_regex_condition() {
    let alert1 = create_test_alert("www.example.com");
    let alert2 = create_test_alert("app.example.org");
    let alert3 = create_test_alert("other.net");

    let rule_content = r#"
- name: "Match Example"
  all:
    - domain_regex: "\\.example\\.com$"
- name: "Match Example"
  all:
    - domain_regex: "\\.example\\.org$"
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };
    let matcher = RuleMatcher::new(&config).unwrap();
    let matches1 = matcher.matches(&alert1, EnrichmentLevel::None);
    assert_eq!(matches1, vec!["Match Example"]);

    let matches2 = matcher.matches(&alert2, EnrichmentLevel::None);
    assert_eq!(matches2, vec!["Match Example"]);

    let matches3 = matcher.matches(&alert3, EnrichmentLevel::None);
    assert!(matches3.is_empty());
}

// TODO: Add back tests for ASN, IP Network, and their 'not' variants
// once they are supported by the new compiled Rule struct.