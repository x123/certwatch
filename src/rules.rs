//! The advanced, staged rule-based filtering engine.
//!
//! This module contains the data structures and logic for parsing, validating,
//! and evaluating complex filtering rules.

use crate::{config::RulesConfig, core::Alert};
use anyhow::{Context, Result};
use ipnetwork::IpNetwork;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use std::{collections::HashSet, fs, net::IpAddr};

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

        let (stage_1_rules, stage_2_rules) =
            all_rules.into_iter().partition(|rule| {
                rule.required_level() == EnrichmentLevel::None
            });

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
            .filter(|rule| rule.expression.evaluate(alert))
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

/// A single, named filtering rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// The name of the rule, for logging and identification.
    pub name: String,
    /// The logical expression that defines the rule's conditions.
    #[serde(flatten)]
    pub expression: RuleExpression,
}

impl Rule {
    /// Loads all rules from the file paths specified in the configuration.
    ///
    /// This function reads each file, parses the YAML content, and aggregates
    /// the rules into a single vector.
    pub fn load_from_files(config: &RulesConfig) -> Result<Vec<Rule>> {
        let mut all_rules = Vec::new();
        for file_path in &config.rule_files {
            let file_content = fs::read_to_string(file_path)
                .with_context(|| format!("Failed to read rule file: {}", file_path.display()))?;

            let rules: Vec<Rule> = serde_yml::from_str(&file_content)
                .with_context(|| format!("Failed to parse YAML from rule file: {}", file_path.display()))?;

            all_rules.extend(rules);
        }
        Ok(all_rules)
    }

    /// Determines the minimum enrichment level required to evaluate this rule.
    pub fn required_level(&self) -> EnrichmentLevel {
        self.expression.required_level()
    }
}

/// The logical expression that makes up a rule.
///
/// This enum represents the nodes in a rule's expression tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleExpression {
    /// A container for conditions that must all be true (AND logic).
    All(Vec<Condition>),
}

impl RuleExpression {
    /// Determines the minimum enrichment level required to evaluate this expression.
    pub fn required_level(&self) -> EnrichmentLevel {
        match self {
            RuleExpression::All(conditions) => conditions
                .iter()
                .map(|c| c.required_level())
                .max()
                .unwrap_or(EnrichmentLevel::None),
        }
    }

    /// Evaluates the expression against an alert.
    pub fn evaluate(&self, alert: &Alert) -> bool {
        let _span = tracing::info_span!("rule_expression_evaluate").entered();
        match self {
            RuleExpression::All(conditions) => {
                conditions.iter().all(|c| c.evaluate(alert))
            }
        }
    }
}

/// A single condition within a rule expression.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Condition {
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_regex: Option<Regex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asns: Option<HashSet<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_networks: Option<Vec<IpNetwork>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_asns: Option<HashSet<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_ip_networks: Option<Vec<IpNetwork>>,
}

impl Condition {
    /// Determines the minimum enrichment level required to evaluate this condition.
    pub fn required_level(&self) -> EnrichmentLevel {
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

    /// Evaluates the condition against an alert.
    pub fn evaluate(&self, alert: &Alert) -> bool {
        let _span = tracing::info_span!("condition_evaluate").entered();
        if let Some(regex) = &self.domain_regex {
            if !regex.is_match(&alert.domain) {
                return false;
            }
        }
        if let Some(asns) = &self.asns {
            if !alert
                .enrichment
                .iter()
                .any(|e| e.asn_info.as_ref().map_or(false, |asn| asns.contains(&asn.as_number)))
            {
                return false;
            }
        }
        if let Some(asns) = &self.not_asns {
            if alert
                .enrichment
                .iter()
                .any(|e| e.asn_info.as_ref().map_or(false, |asn| asns.contains(&asn.as_number)))
            {
                return false;
            }
        }
        if let Some(networks) = &self.ip_networks {
            let all_ips: Vec<IpAddr> = alert.all_ips();
            if !all_ips
                .iter()
                .any(|ip| networks.iter().any(|net| net.contains(*ip)))
            {
                return false;
            }
        }
        if let Some(networks) = &self.not_ip_networks {
            let all_ips: Vec<IpAddr> = alert.all_ips();
            if all_ips
                .iter()
                .any(|ip| networks.iter().any(|net| net.contains(*ip)))
            {
                return false;
            }
        }
        true
    }
}