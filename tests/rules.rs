use certwatch::{
    config::RulesConfig,
    core::{Alert, AsnInfo, DnsInfo, EnrichmentInfo},
    rules::{EnrichmentLevel, Rule, RuleMatcher},
};
use std::{
    collections::HashSet,
    fs::File,
    io::Write,
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
};
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

fn create_test_alert(domain: &str, ips: Vec<IpAddr>, asns: Vec<(u32, &str)>) -> Alert {
    let enrichment = ips
        .iter()
        .zip(asns.iter())
        .map(|(ip, (as_number, as_name))| EnrichmentInfo {
            ip: *ip,
            asn_info: Some(AsnInfo {
                as_number: *as_number,
                as_name: as_name.to_string(),
                country_code: None,
            }),
        })
        .collect();

    Alert {
        domain: domain.to_string(),
        dns: DnsInfo {
            a_records: ips.iter().filter(|ip| ip.is_ipv4()).cloned().collect(),
            aaaa_records: ips.iter().filter(|ip| ip.is_ipv6()).cloned().collect(),
            ..Default::default()
        },
        enrichment,
        ..Default::default()
    }
}

#[test]
fn test_load_rules_from_files() {
    let rule_content = r#"
- name: "Test Rule 1"
  all:
    - domain_regex: "^test1\\.com$"
- name: "Test Rule 2"
  all:
    - domain_regex: "^test2\\.com$"
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };

    let rules = Rule::load_from_files(&config).unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].name, "Test Rule 1");
    assert_eq!(rules[1].name, "Test Rule 2");
}

#[test]
fn test_rule_classification() {
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
fn test_domain_regex_condition() {
    let alert = create_test_alert("www.example.com", vec![], vec![]);
    let rule_content = r#"
- name: "Match Example"
  all:
    - domain_regex: "^www\\.example\\.com$"
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };
    let matcher = RuleMatcher::new(&config).unwrap();
    let matches = matcher.matches(&alert, EnrichmentLevel::None);
    assert_eq!(matches, vec!["Match Example"]);
}

#[test]
fn test_asn_condition() {
    let alert = create_test_alert(
        "test.com",
        vec![Ipv4Addr::new(8, 8, 8, 8).into()],
        vec![(15169, "Google LLC")],
    );
    let rule_content = r#"
- name: "Match Google ASN"
  all:
    - asns: [15169, 12345]
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };
    let matcher = RuleMatcher::new(&config).unwrap();
    let matches = matcher.matches(&alert, EnrichmentLevel::Standard);
    assert_eq!(matches, vec!["Match Google ASN"]);
}

#[test]
fn test_not_asn_condition() {
    let alert = create_test_alert(
        "test.com",
        vec![Ipv4Addr::new(8, 8, 8, 8).into()],
        vec![(15169, "Google LLC")],
    );
    let rule_content = r#"
- name: "Not Cloudflare"
  all:
    - not_asns: [13335]
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };
    let matcher = RuleMatcher::new(&config).unwrap();
    let matches = matcher.matches(&alert, EnrichmentLevel::Standard);
    assert_eq!(matches, vec!["Not Cloudflare"]);
}

#[test]
fn test_ip_network_condition() {
    let alert = create_test_alert(
        "test.com",
        vec![Ipv4Addr::new(192, 168, 1, 50).into()],
        vec![(123, "Private")],
    );
    let rule_content = r#"
- name: "Private Network"
  all:
    - ip_networks: ["192.168.1.0/24"]
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };
    let matcher = RuleMatcher::new(&config).unwrap();
    let matches = matcher.matches(&alert, EnrichmentLevel::Standard);
    assert_eq!(matches, vec!["Private Network"]);
}

#[test]
fn test_not_ip_network_condition() {
    let alert = create_test_alert(
        "test.com",
        vec![Ipv4Addr::new(8, 8, 8, 8).into()],
        vec![(15169, "Google LLC")],
    );
    let rule_content = r#"
- name: "Not Private Network"
  all:
    - not_ip_networks: ["192.168.0.0/16", "10.0.0.0/8"]
"#;
    let rule_file = create_rule_file(rule_content);
    let config = RulesConfig {
        rule_files: vec![rule_file],
    };
    let matcher = RuleMatcher::new(&config).unwrap();
    let matches = matcher.matches(&alert, EnrichmentLevel::Standard);
    assert_eq!(matches, vec!["Not Private Network"]);
}