#![allow(dead_code)]
//! A mock pattern matcher for testing purposes.

use async_trait::async_trait;
use certwatch::core::PatternMatcher;

pub struct MockPatternMatcher;
#[async_trait]
impl PatternMatcher for MockPatternMatcher {
    async fn match_domain(&self, _domain: &str) -> Option<String> {
        Some("mock_tag".to_string())
    }
}