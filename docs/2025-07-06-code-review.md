# Code Review: 2025-07-06

## 1. Overview

This review assesses the current state of the `certwatch` codebase against the objectives defined in `docs/specs.md` and the implementation history in `docs/plan.md`. The goal is to identify technical debt, feature gaps, and areas for improvement before starting the next development cycle.

## 2. High-Level Assessment

The project is in a strong state. The architecture is decoupled and testable, successfully leveraging traits for dependency injection. The core features outlined in the initial epics are largely complete and functional. The recent addition of the `TsvAsnLookup` provider was well-executed and followed the established architectural patterns.

However, the review identifies several feature gaps from the specification and remaining technical debt from previous refactoring cycles that should be addressed.

## 3. Analysis Against `specs.md`

### 3.1. Feature Gaps & Incomplete Implementations

*   **[HIGH] Configurable Sampling (Spec §2.1):** The `CertStreamClient` does not yet implement the required sampling feature. The `sample_rate` field is present in the configuration but is not used. This is a critical feature for managing performance on high-volume streams. This was planned in **Epic 4.5, Task #B** but is not complete.

*   **[MEDIUM] Self-Signed Certificate Handling (Spec Implicit):** The TLS connection logic in `network.rs` was hardcoded to allow invalid certificates during a debugging session. This is a security risk and should be an explicit, user-configurable option. This was planned in **Epic 4.5, Task #D** but is not complete.

*   **[LOW] Live Metrics Display (Spec §4):** The `--live-metrics` flag is defined as a command-line option but there is no implementation to display real-time metrics. This is a non-critical feature but is required by the specification.

### 3.2. Areas of Technical Debt

*   **[MEDIUM] Duplicated Message Parsing Logic in `network.rs`:** The `network.rs` module contains two very similar message parsing functions. This duplication was likely introduced during the initial build-out and subsequent debugging. It increases maintenance overhead and should be refactored into a single, robust function. This was planned in **Epic 4.5, Task #D**.

*   **[LOW] Inconsistent Naming in Enrichment Traits:**
    *   The ASN lookup trait is named `EnrichmentProvider`, but the GeoIP lookup trait is `GeoIpLookup`.
    *   The ASN implementation is `TsvAsnLookup`, but the GeoIP one is `MaxmindGeoIpProvider`.
    *   This should be standardized to `T` for traits and `Provider` for implementations (e.g., `AsnProvider` trait, `TsvAsnProvider` struct).

*   **[LOW] `println!` in `matching.rs`:** The `PatternWatcher` uses `println!` for logging status updates. This bypasses the application's logging framework (`log` crate) and prevents consistent log formatting and filtering. This was planned in **Epic 4.5, Task #D**.

*   **[LOW] Confusing `PatternWatcher` API:** The `new` function for `PatternWatcher` takes a `HashMap` of patterns, which is immediately converted into a `Vec`. The API could be simplified by accepting a `Vec` directly, making the caller's responsibility clearer. This was planned in **Epic 4.5, Task #D**.

### 3.3. Insufficient Testing

*   **[MEDIUM] Missing Integration Test for TSV Provider:** While the `TsvAsnLookup` has excellent unit tests, there is no corresponding integration test in the `/tests` directory to verify its behavior within the full application pipeline, as was done for other components. **Epic 7, Task #13** specified this, but it appears to have been missed. A live test similar to `tests/live_enrichment.rs` should be created.

## 4. Summary of Unfinished Work from `plan.md`

The following tasks from **Epic 4.5: Pre-Flight Refactoring** remain incomplete:

*   **#B - Implement Configurable Sampling:** `sample_rate` config exists but is not used.
*   **#D - General Code Cleanup:**
    *   Refactor duplicated message handling in `network.rs`.
    *   Replace `println!` in `matching.rs`.
    *   Simplify `PatternWatcher` API.
    *   Make self-signed certificate handling configurable.

## 5. Recommendations & Proposed Plan

Based on this review, I propose the following plan to address the identified issues. This plan prioritizes closing feature gaps from the spec and paying down the most significant technical debt.

1.  **Implement Configurable Sampling:**
    *   Modify `CertStreamClient` in `src/network.rs` to read the `sample_rate` from the configuration.
    *   Add logic to the message processing loop to randomly select a percentage of incoming domains.
    *   Add a unit test to verify the sampling logic.

2.  **Refactor Network Code & Add Configuration:**
    *   Make the `allow_invalid_certs` flag in `src/network.rs` a boolean field in the main `Config` struct.
    *   Refactor the duplicated message parsing functions into a single private helper function.

3.  **Improve Testing for TSV Provider:**
    *   Create a new integration test `tests/live_tsv_enrichment.rs`.
    *   This test will configure the application to use the `Tsv` provider and run the full pipeline against the live `certstream` feed, asserting that enrichment occurs correctly.

4.  **Address Minor Technical Debt:**
    *   Change `println!` calls in `src/matching.rs` to `log::info!`.
    *   Update the `PatternWatcher::new()` signature to accept a `Vec` instead of a `HashMap`.
    *   Standardize the naming of enrichment traits and structs.

Completing these tasks will bring the application into full compliance with the specification and significantly improve its maintainability.