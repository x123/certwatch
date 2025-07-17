# Development Plan

## Epic: Implement Pre-DNS Rule Filtering

**User Story:** As an operator, I want the application to filter incoming domains from the `CertStream` against a set of pre-defined rules *before* performing any expensive DNS lookups, so that system resources are conserved and the application can handle high-volume streams efficiently without dropping legitimate domains of interest.

### Tasks

1.  **Create a Failing Integration Test:**
    *   **Goal:** Write a new integration test in `tests/integrations/pre_filtering.rs` that simulates a high volume of mixed domains (some matching, some not).
    *   **Assertions:**
        *   The test will use a mock DNS resolver that counts the number of times `resolve()` is called.
        *   It will assert that `resolve()` is only called for the domains that match the pre-filtering rules.
        *   It will assert that the `certwatch_domains_prefiltered_total` metric is incremented correctly for the dropped domains.
    *   **Initial State:** This test will fail because the pre-filtering logic does not yet exist.

2.  **Expose the Pre-Filtering Method:**
    *   **Goal:** Investigate `src/rules.rs` to find the existing method for Stage 1 filtering (likely based on `EnrichmentLevel::None`).
    *   **Action:** Make this method public so it can be called from `src/app.rs`.

3.  **Implement the Pre-Filtering Logic:**
    *   **Goal:** Modify the `dns_manager_task` in `src/app.rs` to perform pre-filtering.
    *   **Actions:**
        *   Inject the `RuleMatcher` into the task.
        *   For each domain received from the `CertStream`, call the pre-filtering method.
        *   If the domain matches, forward it to the `DnsResolutionManager`.
        *   If it does not match, drop it and increment the new `certwatch_domains_prefiltered_total` metric.

4.  **Add the New Metric:**
    *   **Goal:** Add the `certwatch_domains_prefiltered_total` counter to the metrics system.
    *   **Action:** Update `src/internal_metrics/mod.rs` to include the new metric.

5.  **Verify the Fix:**
    *   **Goal:** Run the new integration test and ensure it passes.
    *   **Action:** Execute `cargo nextest run --test pre_filtering`.

6.  **Cleanup and Final Validation:**
    *   **Goal:** Ensure the codebase is clean and all tests pass.
    *   **Actions:**
        *   Remove any temporary code or comments.
        *   Run the full test suite with `just test`.
