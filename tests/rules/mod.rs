use certwatch::{
    config::RulesConfig,
    core::{Alert, DnsInfo},
    internal_metrics::Metrics,
    rules::{EnrichmentLevel, RuleLoader, RuleMatcher},
};
use std::sync::Arc;

#[path = "../helpers/mod.rs"]
mod helpers;
use helpers::test_utils::create_rule_file;

fn create_test_alert(domain: &str) -> Alert {
    Alert {
        domain: domain.to_string(),
        dns: DnsInfo::default(),
        enrichment: Vec::new(),
        ..Default::default()
    }
}

#[test]
fn test_rule_classification() {
    let rule_content = r#"
rules:
  - name: "Stage 1 Rule"
    all:
      - domain_regex: "stage1"
  - name: "Stage 2 Rule"
    any:
      - asns: [123]
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file].into(),
    };
    let rule_set = RuleLoader::load_from_files(&config).unwrap();
    let matcher = RuleMatcher::new(rule_set, Arc::new(Metrics::new_for_test())).unwrap();

    assert_eq!(matcher.stage_1_rules.len(), 1);
    assert_eq!(matcher.stage_1_rules[0].name, "Stage 1 Rule");
    assert_eq!(matcher.stage_2_rules.len(), 1);
    assert_eq!(matcher.stage_2_rules[0].name, "Stage 2 Rule");
}

#[test]
fn test_boolean_logic_evaluation() {
    let rule_content = r#"
rules:
  - name: "Simple ALL Match"
    all:
      - domain_regex: "^test\\.com$"
      - domain_regex: "test"
  - name: "Simple ALL Fail"
    all:
      - domain_regex: "^test\\.com$"
      - domain_regex: "fail"
  - name: "Simple ANY Match"
    any:
      - domain_regex: "^test\\.com$"
      - domain_regex: "fail"
  - name: "Nested Match"
    all:
      - domain_regex: "test"
      - any:
        - domain_regex: "fail"
        - asns: [123]
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file].into(),
    };
    let rule_set = RuleLoader::load_from_files(&config).unwrap();
    let matcher = RuleMatcher::new(rule_set, Arc::new(Metrics::new_for_test())).unwrap();

    let mut alert = create_test_alert("test.com");
    alert.enrichment.push(certwatch::core::EnrichmentInfo {
        ip: "1.1.1.1".parse().unwrap(),
        asn_info: Some(certwatch::core::AsnInfo {
            as_number: 123,
            as_name: "Test ASN".to_string(),
            country_code: Some("US".to_string()),
        }),
    });

    // Test Stage 1 rules (no enrichment needed)
    let matches_s1 = matcher.matches(&alert, EnrichmentLevel::None);
    assert_eq!(matches_s1.len(), 2);
    assert!(matches_s1.contains(&"Simple ALL Match".to_string()));
    assert!(matches_s1.contains(&"Simple ANY Match".to_string()));

    // Test Stage 2 rules (enrichment needed)
    let matches_s2 = matcher.matches(&alert, EnrichmentLevel::Standard);
    assert_eq!(matches_s2.len(), 1);
    assert!(matches_s2.contains(&"Nested Match".to_string()));
}

#[test]
fn test_prefilter_ignore_integration() {
    let rule_content = r#"
ignore:
  - "\\.ignored\\.com$"
  - "exact-ignore.net"

rules:
  - name: "Should Not Be Matched"
    all:
      - domain_regex: ".*"
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file].into(),
    };
    let rule_set = RuleLoader::load_from_files(&config).unwrap();
    let matcher = RuleMatcher::new(rule_set, Arc::new(Metrics::new_for_test())).unwrap();

    assert!(
        matcher.is_ignored("sub.ignored.com"),
        "Subdomain should be ignored"
    );
    assert!(
        matcher.is_ignored("exact-ignore.net"),
        "Exact domain should be ignored"
    );
    assert!(
        !matcher.is_ignored("safe-domain.com"),
        "Safe domain should not be ignored"
    );
}

#[test]
fn test_prefilter_no_ignore_list() {
    let rule_content = r#"
rules:
  - name: "Some Rule"
    all:
      - domain_regex: ".*"
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file].into(),
    };
    let rule_set = RuleLoader::load_from_files(&config).unwrap();
    let matcher = RuleMatcher::new(rule_set, Arc::new(Metrics::new_for_test())).unwrap();

    assert!(!matcher.is_ignored("anything.com"));
}

#[test]
fn test_prefilter_empty_ignore_list() {
    let rule_content = r#"
ignore: []
rules:
  - name: "Some Rule"
    all:
      - domain_regex: ".*"
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file].into(),
    };
    let rule_set = RuleLoader::load_from_files(&config).unwrap();
    let matcher = RuleMatcher::new(rule_set, Arc::new(Metrics::new_for_test())).unwrap();

    assert!(!matcher.is_ignored("anything.com"));
}

#[test]
fn test_prefilter_invalid_regex_fails_load() {
    let rule_content = r#"
ignore:
  - "(" # Invalid regex
rules: []
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file].into(),
    };
    let rule_set = RuleLoader::load_from_files(&config).unwrap();
    let result = RuleMatcher::new(rule_set, Arc::new(Metrics::new_for_test()));
    assert!(result.is_err());
}
