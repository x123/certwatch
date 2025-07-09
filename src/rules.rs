//! The advanced, staged rule-based filtering engine.
//!
//! This module contains the data structures and logic for parsing, validating,
//! and evaluating complex filtering rules.

use crate::{config::RulesConfig, core::Alert};
use anyhow::{Context, Result};
use ipnetwork::IpNetwork;
use regex::RegexSet;
use serde::{Deserialize, Serialize};
use std::fs;

/// A pre-emptive filter that uses a `RegexSet` to quickly discard domains
/// that match a global ignore list.
#[derive(Debug, Clone)]
pub struct PreFilter {
    ignore_set: Option<RegexSet>,
    metric_domains_ignored: metrics::Counter,
}

impl PreFilter {
    /// Creates a new `PreFilter` from a list of ignore patterns.
    ///
    /// If the list of patterns is empty or `None`, it creates a filter that
    /// will never match.
    pub fn new(patterns: Option<Vec<String>>) -> Result<Self> {
        let ignore_set = if let Some(patterns) = patterns {
            if patterns.is_empty() {
                None
            } else {
                Some(RegexSet::new(patterns)?)
            }
        } else {
            None
        };

        Ok(Self {
            ignore_set,
            metric_domains_ignored: metrics::counter!("certwatch_domains_ignored_total"),
        })
    }

    /// Checks if a domain should be ignored.
    ///
    /// Returns `true` if the domain matches the ignore list, `false` otherwise.
    pub fn is_match(&self, domain: &str) -> bool {
        if let Some(set) = &self.ignore_set {
            let is_match = set.is_match(domain);
            if is_match {
                self.metric_domains_ignored.increment(1);
                tracing::debug!(domain, "Domain matched ignore list");
            }
            is_match
        } else {
            false
        }
    }
}

/// A container for the staged rule sets.
#[derive(Debug, Clone)]
pub struct RuleMatcher {
    /// Rules that can be evaluated before enrichment.
    pub stage_1_rules: Vec<Rule>,
    /// Rules that require enrichment data to be evaluated.
    pub stage_2_rules: Vec<Rule>,
    /// The pre-emptive filter for ignoring domains.
    pre_filter: PreFilter,
}

impl RuleMatcher {
    /// Loads rules and ignore patterns from the config, returning a `RuleMatcher`
    /// that contains all compiled rules and the pre-filter.
    pub fn load(config: &RulesConfig) -> Result<Self> {
        let (all_rules, ignore_patterns) = Rule::load_from_files(config)?;

        let (stage_2_rules, stage_1_rules) = all_rules
            .into_iter()
            .partition(|rule| rule.required_level == EnrichmentLevel::Standard);

        let pre_filter = PreFilter::new(Some(ignore_patterns))?;

        Ok(Self {
            stage_1_rules,
            stage_2_rules,
            pre_filter,
        })
    }

    /// Checks if a domain should be ignored by the pre-filter.
    pub fn is_ignored(&self, domain: &str) -> bool {
        self.pre_filter.is_match(domain)
    }

    /// Checks an alert against the rules for a given enrichment stage.
    /// Returns a list of names of the rules that matched.
    pub fn matches(&self, alert: &Alert, level: EnrichmentLevel) -> Vec<String> {
        let _span = tracing::info_span!("rule_matcher_matches", ?level).entered();
        let rules_to_check = match level {
            EnrichmentLevel::None => &self.stage_1_rules,
            EnrichmentLevel::Standard => &self.stage_2_rules,
        };

        rules_to_check
            .iter()
            .filter(|rule| rule.is_match(alert))
            .map(|rule| rule.name.clone())
            .collect()
    }

    #[cfg(test)]
    pub fn new_for_test(rules: Vec<Rule>) -> Self {
        let (stage_2_rules, stage_1_rules) = rules
            .into_iter()
            .partition(|rule| rule.required_level == EnrichmentLevel::Standard);

        Self {
            stage_1_rules,
            stage_2_rules,
            pre_filter: PreFilter::new(None).unwrap(),
        }
    }
}

/// The level of data enrichment required to evaluate a rule or condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnrichmentLevel {
    /// Requires only the base alert data (domain name).
    None,
    /// Requires standard enrichment (DNS, ASN).
    Standard,
}

/// A single, named filtering rule that has been compiled for efficient matching.
#[derive(Debug, Clone)]
pub struct Rule {
    /// The name of the rule, for logging and identification.
    pub name: String,
    /// The compiled expression tree for this rule.
    pub expression: RuleExpression,
    /// The enrichment level required for this rule.
    pub required_level: EnrichmentLevel,
}

impl Rule {
    /// Recursively evaluates the rule's expression tree against an alert.
    pub fn is_match(&self, alert: &Alert) -> bool {
        Self::evaluate_expression(&self.expression, alert)
    }

    /// The core recursive evaluation logic.
    fn evaluate_expression(expression: &RuleExpression, alert: &Alert) -> bool {
        match expression {
            RuleExpression::All(expressions) => expressions
                .iter()
                .all(|expr| Self::evaluate_expression(expr, alert)),
            RuleExpression::Any(expressions) => expressions
                .iter()
                .any(|expr| Self::evaluate_expression(expr, alert)),
            RuleExpression::DomainRegex(pattern) => {
                // This is inefficient as it recompiles regex on every check.
                // A future optimization will be to pre-compile these.
                match regex::Regex::new(pattern) {
                    Ok(re) => re.is_match(&alert.domain),
                    Err(_) => false, // Invalid patterns were already filtered during loading.
                }
            }
            RuleExpression::Asns(asns) => alert
                .enrichment
                .iter()
                .filter_map(|e| e.asn_info.as_ref())
                .any(|info| asns.contains(&info.as_number)),
            RuleExpression::NotAsns(asns) => {
                let has_enrichment = alert.enrichment.iter().any(|e| e.asn_info.is_some());
                if !has_enrichment {
                    return false; // Cannot satisfy a 'not' if there's nothing to check.
                }
                !alert
                    .enrichment
                    .iter()
                    .filter_map(|e| e.asn_info.as_ref())
                    .any(|info| asns.contains(&info.as_number))
            }
            RuleExpression::IpNetworks(networks) => {
                let all_ips = alert.all_ips();
                networks.iter().any(|net| all_ips.iter().any(|ip| net.contains(*ip)))
            }
            RuleExpression::NotIpNetworks(networks) => {
                let all_ips = alert.all_ips();
                if all_ips.is_empty() {
                    return false; // Cannot satisfy a 'not' if there's nothing to check.
                }
                !networks.iter().any(|net| all_ips.iter().any(|ip| net.contains(*ip)))
            }
        }
    }

    /// Loads all rules from the file paths specified in the configuration.
    pub fn load_from_files(config: &RulesConfig) -> Result<(Vec<Rule>, Vec<String>)> {
        let mut compiled_rules = Vec::new();
        let mut ignore_patterns = Vec::new();

        if let Some(rule_files) = &config.rule_files {
            for file_path in rule_files {
                let file_content = fs::read_to_string(file_path)
                    .with_context(|| format!("Failed to read rule file: {}", file_path.display()))?;

                let rules_file: RulesFile = serde_yml::from_str(&file_content).with_context(|| {
                    format!(
                        "Failed to parse YAML from rule file: {}",
                        file_path.display()
                    )
                })?;

                if let Some(patterns) = rules_file.ignore {
                    ignore_patterns.extend(patterns);
                }

                for file_rule in rules_file.rules {
                    // Basic validation for now. A more robust validation step can be added.
                    // For example, ensuring regexes are valid.
                    let required_level = file_rule.expression.required_level();
                    compiled_rules.push(Rule {
                        name: file_rule.name,
                        expression: file_rule.expression,
                        required_level,
                    });
                }
            }
        }

        Ok((compiled_rules, ignore_patterns))
    }
}

// --- Deserialization-only structs ---

/// Represents the top-level structure of a rule file.
#[derive(Debug, Serialize, Deserialize)]
struct RulesFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ignore: Option<Vec<String>>,
    #[serde(default)]
    rules: Vec<FileRule>,
}

/// A temporary struct that represents a rule as defined in the YAML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileRule {
    name: String,
    #[serde(flatten)]
    expression: RuleExpression,
}

/// A recursive enum representing the boolean logic of a rule.
/// This is used for deserializing from the YAML files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleExpression {
    All(Vec<RuleExpression>),
    Any(Vec<RuleExpression>),
    DomainRegex(String),
    Asns(Vec<u32>),
    IpNetworks(Vec<IpNetwork>),
    NotAsns(Vec<u32>),
    NotIpNetworks(Vec<IpNetwork>),
}

impl RuleExpression {
    /// Determines the minimum enrichment level required to evaluate this expression.
    fn required_level(&self) -> EnrichmentLevel {
        match self {
            RuleExpression::All(exprs) | RuleExpression::Any(exprs) => exprs
                .iter()
                .map(|e| e.required_level())
                .max()
                .unwrap_or(EnrichmentLevel::None),
            RuleExpression::Asns(_)
            | RuleExpression::IpNetworks(_)
            | RuleExpression::NotAsns(_)
            | RuleExpression::NotIpNetworks(_) => EnrichmentLevel::Standard,
            RuleExpression::DomainRegex(_) => EnrichmentLevel::None,
        }
    }
}