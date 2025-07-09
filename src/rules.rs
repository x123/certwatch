//! The advanced, staged rule-based filtering engine.
//!
//! This module contains the data structures and logic for parsing, validating,
//! and evaluating complex filtering rules.

use crate::{config::RulesConfig, core::Alert};
use anyhow::{Context, Result};
use ipnetwork::IpNetwork;
use itertools::Itertools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{fs};

/// A container for the staged rule sets.
#[derive(Debug, Clone)]
pub struct RuleMatcher {
    /// Rules that can be evaluated before enrichment.
    pub stage_1_rules: Vec<Rule>,
    /// Rules that require enrichment data to be evaluated.
    pub stage_2_rules: Vec<Rule>,
}

impl RuleMatcher {
    /// Creates a new `RuleMatcher` by loading and classifying rules from the config.
    pub fn new(config: &RulesConfig) -> Result<Self> {
        let all_rules = Rule::load_from_files(config)?;

        let (stage_2_rules, stage_1_rules) = all_rules
            .into_iter()
            .partition(|rule| rule.required_level == EnrichmentLevel::Standard);

        Ok(Self {
            stage_1_rules,
            stage_2_rules,
        })
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
    /// The compiled regular expression for all domain-based conditions.
    pub domain_regex: Option<Regex>,
    // Other compiled conditions will go here in the future.
    /// The enrichment level required for this rule.
    pub required_level: EnrichmentLevel,
}

impl Rule {
    /// Evaluates the rule against an alert.
    pub fn is_match(&self, alert: &Alert) -> bool {
        if let Some(regex) = &self.domain_regex {
            if !regex.is_match(&alert.domain) {
                return false;
            }
        }
        // In the future, we'll check other conditions here.
        true
    }

    /// Loads all rules from the file paths specified in the configuration,
    /// grouping and compiling them for efficiency.
    pub fn load_from_files(config: &RulesConfig) -> Result<Vec<Rule>> {
        let mut raw_rules = Vec::new();
        for file_path in &config.rule_files {
            let file_content = fs::read_to_string(file_path)
                .with_context(|| format!("Failed to read rule file: {}", file_path.display()))?;

            let rules: Vec<FileRule> = serde_yml::from_str(&file_content).with_context(|| {
                format!(
                    "Failed to parse YAML from rule file: {}",
                    file_path.display()
                )
            })?;
            raw_rules.extend(rules);
        }

        // Group rules by name and compile their regexes
        let mut compiled_rules = Vec::new();
        raw_rules.sort_by(|a, b| a.name.cmp(&b.name));

        for (name, group) in &raw_rules.into_iter().chunk_by(|r| r.name.clone()) {
            let mut domain_patterns = Vec::new();
            let mut required_level = EnrichmentLevel::None;

            for item in group {
                if let Some(cond) = item.expression.all.first() {
                    if let Some(pattern) = &cond.domain_regex {
                        domain_patterns.push(pattern.clone());
                    }
                    if cond.required_level() > required_level {
                        required_level = cond.required_level();
                    }
                }
            }

            let domain_regex = if !domain_patterns.is_empty() {
                // Combine all patterns into a single regex: (p1)|(p2)|...
                let combined = domain_patterns
                    .into_iter()
                    .map(|p| format!("({})", p))
                    .join("|");
                let regex = Regex::new(&combined).with_context(|| {
                    format!("Failed to compile combined regex for rule '{}'", name)
                })?;
                Some(regex)
            } else {
                None
            };

            compiled_rules.push(Rule {
                name,
                domain_regex,
                required_level,
            });
        }

        Ok(compiled_rules)
    }
}

// --- Deserialization-only structs ---

/// A temporary struct that represents a rule as defined in the YAML file.
#[derive(Debug, Serialize, Deserialize)]
struct FileRule {
    name: String,
    #[serde(flatten)]
    expression: FileRuleExpression,
}

/// A temporary struct for deserializing the `all` block.
#[derive(Debug, Serialize, Deserialize)]
struct FileRuleExpression {
    all: Vec<FileCondition>,
}

/// A temporary struct for deserializing a single condition.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct FileCondition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    domain_regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asns: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ip_networks: Option<Vec<IpNetwork>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    not_asns: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    not_ip_networks: Option<Vec<IpNetwork>>,
}

impl FileCondition {
    /// Determines the minimum enrichment level required to evaluate this condition.
    fn required_level(&self) -> EnrichmentLevel {
        if self.asns.is_some()
            || self.ip_networks.is_some()
            || self.not_asns.is_some()
            || self.not_ip_networks.is_some()
        {
            EnrichmentLevel::Standard
        } else {
            EnrichmentLevel::None
        }
    }
}