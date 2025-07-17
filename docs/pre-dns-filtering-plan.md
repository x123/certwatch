# Pre-DNS Rule Filtering: Implementation Plan

This document outlines a safe, incremental strategy for implementing pre-DNS rule filtering. The goal is to improve efficiency by dropping domains that match certain rules *before* they undergo costly DNS resolution, without breaking the existing test suite.

This plan adheres strictly to the "Golden Rules of Integration Testing" defined in the project's `.clinerules`.

## 1. Strategy for New Integration Test

We will create a new, failing integration test in a new file: `tests/integrations/pre_dns_filtering.rs`. This test will drive the implementation.

**Test Objective:** Verify that a domain matching a `DomainRegex` rule is filtered out *before* it reaches the DNS resolver, and that the `certwatch_domains_prefiltered_total` metric is incremented.

**Test Implementation (`tests/integrations/pre_dns_filtering.rs`):**

```rust
use anyhow::Result;
use certwatch::internal_metrics::Metrics;
use std::sync::Arc;
use std::time::Duration;
use tests::helpers::app::TestAppBuilder;
use tests::helpers::mock_dns::MockDnsResolver;

#[tokio::test]
async fn test_domain_is_filtered_before_dns() -> Result<()> {
    // 1. Setup: Create a mock DNS resolver that will panic if called.
    let dns_resolver = Arc::new(MockDnsResolver::new().with_panic_on_resolve());

    // 2. Setup: Define a simple rule that will match the test domain.
    let rules = r#"
rules:
  - name: "block-test-domain"
    domain_regex: "^test-domain\\.com$"
"#;

    // 3. Setup: Initialize the TestApp with the panicking resolver and the rule.
    let test_app = TestAppBuilder::new()
        .with_test_domains_channel()
        .with_dns_resolver(dns_resolver)
        .with_rules(rules)
        .with_metrics() // Enable metrics
        .start()
        .await?;

    // 4. Act: Send the domain that should be filtered.
    test_app.send_domain("test-domain.com").await?;
    test_app.send_domain("another-domain.com").await?; // This one should pass through

    // 5. Assert: Give the app a moment to process, then check the metric.
    // The test will fail here initially because the metric doesn't exist.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let metrics_body = reqwest::get(&format!("http://{}/metrics", test_app.metrics_addr()))
        .await?
        .text()
        .await?;

    assert!(
        metrics_body.contains(r#"certwatch_domains_prefiltered_total 1"#),
        "Metric 'certwatch_domains_prefiltered_total' should be 1. Body was:\n{}",
        metrics_body
    );

    // The mock DNS resolver will panic if `resolve` is called for "test-domain.com",
    // causing the test to fail if the pre-filtering logic is not working.

    // 6. Teardown
    test_app.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
```

This test will initially fail because:
1. The `certwatch_domains_prefiltered_total` metric does not exist.
2. The application logic does not yet perform pre-filtering.
3. The `MockDnsResolver` will panic when `another-domain.com` is sent for resolution, because we haven't configured a response for it. We will adjust the mock to handle this.

## 2. Exposing Filtering Logic from `src/rules.rs`

The `RuleMatcher` already has `stage_1_rules` which can be evaluated without DNS info. We need a new public method to check a domain against just these rules.

**Plan:**

1.  **Add a new public method to `RuleMatcher` in `src/rules.rs`:**
    This method will specifically check a domain against `stage_1_rules` (those with `EnrichmentLevel::None`).

    ```rust
    // In src/rules.rs, inside impl RuleMatcher
    
    /// Checks a domain against rules that do not require enrichment.
    /// Returns `true` if any Stage 1 rule matches.
    pub fn matches_pre_dns(&self, domain: &str) -> bool {
        // Create a temporary, minimal Alert for the matching logic.
        let temp_alert = Alert {
            domain: domain.to_string(),
            ..Alert::default() // Fill with default values
        };

        self.stage_1_rules
            .iter()
            .any(|rule| rule.is_match(&temp_alert))
    }
    ```

## 3. Modifying `src/app.rs` for Pre-filtering

The filtering logic will be introduced in the `dns_manager_task` loop in `src/app.rs`, right after a domain is received from the `domains_rx` channel.

**Plan:**

1.  **Inject `RuleMatcher` into the `dns_manager_task`:** The `Arc<RuleMatcher>` is already available in the `build` function. We will clone it and move it into the `tokio::spawn` closure for the `dns_manager_task`.

2.  **Add the filtering logic:**

    ```rust
    // In src/app.rs, inside the dns_manager_task's loop
    
    // ... inside tokio::spawn for dns_manager_task
    let rule_matcher = rule_matcher.clone(); // Get the rule_matcher
    
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                info!("DNS domain forwarder received shutdown signal.");
                break;
            }
            Ok(domain) = domains_rx.recv() => {
                metrics.domains_processed_total.increment(1);

                // <<< NEW FILTERING LOGIC >>>
                if rule_matcher.matches_pre_dns(&domain) {
                    metrics.increment_domains_prefiltered(); // New metric method
                    trace!(domain = %domain, "Domain pre-filtered, skipping DNS resolution.");
                    continue; // Skip to the next domain
                }
                // <<< END NEW LOGIC >>>

                let start_time = std::time::Instant::now();
                metrics::gauge!("domains_queued").decrement(1.0);
                if let Err(e) = dns_manager.resolve(domain, "certstream".to_string(), start_time.into()) {
                    trace!("Failed to send domain to DNS manager: {}", e);
                }
            }
            else => {
                info!("Domain channel closed, DNS forwarder shutting down.");
                break;
            }
        }
    }
    ```

## 4. Adding the New Metric

We need to define the new metric and create a helper method for it.

**Plan:**

1.  **Define the metric in `src/internal_metrics/mod.rs`:**
    -   Add the counter to the `Metrics` struct.
    -   Add the description in `Metrics::new()`.
    -   Initialize it in `Metrics::new()` and `Metrics::disabled()`.

    ```rust
    // In src/internal_metrics/mod.rs

    // In the Metrics struct
    pub domains_prefiltered_total: Counter,

    // In Metrics::new()
    metrics::describe_counter!("certwatch_domains_prefiltered_total", Unit::Count, "Total number of domains that were filtered out by rules before DNS resolution.");
    
    // In the return struct of Metrics::new()
    domains_prefiltered_total: metrics::counter!("certwatch_domains_prefiltered_total"),

    // In Metrics::disabled()
    domains_prefiltered_total: metrics::counter!("disabled"),
    ```

2.  **Create an incrementer method in `src/internal_metrics/mod.rs`:**

    ```rust
    // In src/internal_metrics/mod.rs, inside impl Metrics
    
    /// Increments the counter for pre-filtered domains.
    pub fn increment_domains_prefiltered(&self) {
        self.domains_prefiltered_total.increment(1);
    }
    ```

## 5. Step-by-Step Implementation Sequence

This sequence is designed to keep the test suite passing until the final step.

1.  **Step 1: Metric and Logic Scaffolding (Tests should pass)**
    a. Add the `certwatch_domains_prefiltered_total` metric to `src/internal_metrics/mod.rs` as described above.
    b. Add the public `matches_pre_dns` method to `RuleMatcher` in `src/rules.rs`.
    c. Run `just test` to ensure no regressions were introduced.

2.  **Step 2: Create the Failing Test (Tests will fail)**
    a. Create the new test file `tests/integrations/pre_dns_filtering.rs` with the content from section 1.
    b. Add `mod pre_dns_filtering;` to `tests/integrations/mod.rs`.
    c. Modify the `MockDnsResolver` in `tests/helpers/mock_dns.rs` to accept expected resolutions, so it doesn't panic on "another-domain.com".
    d. Run `just test`. The new test `test_domain_is_filtered_before_dns` should fail because the filtering logic is not yet implemented in `app.rs`.

3.  **Step 3: Implement the Core Logic (Tests should pass)**
    a. Modify `src/app.rs` to include the filtering logic in the `dns_manager_task` as described in section 3.
    b. Run `just test`. All tests, including the new integration test, should now pass.

This incremental approach ensures that we have a failing test that clearly defines our goal, and we only make it pass by implementing the correct logic in the application core.