# Code Review & Tech Debt Analysis: 2025-07-05

## 1. Overview

A deep review of the codebase was conducted to identify technical debt, bugs, and feature gaps before beginning "Epic 5: Output & Alerting". While the project is in a strong state with a solid, testable architecture, several issues were identified.

## 2. Findings

### High-Priority Issues (Feature Gaps & Bugs)

1.  **`dns.rs`: "First Resolution" Alert is Not Implemented (Critical)**
    *   **Problem:** The `nxdomain_retry_task` identifies when a domain resolves after previously being `NXDOMAIN`, but it only logs a message (`// TODO:`). It does not send the result anywhere, making it impossible to generate the "Follow-up 'First Resolution' Alert" required by spec 2.4.
    *   **Impact:** A key feature of the application is missing.

2.  **`network.rs`: Configurable Sampling Rate is Not Implemented**
    *   **Problem:** The `CertStreamClient` processes 100% of incoming domains. The configurable sampling rate required by spec 2.1 is missing.
    *   **Impact:** The application cannot be configured to handle high-volume streams on resource-constrained systems.

3.  **`dns.rs`: Incorrect Error Handling (Bug)**
    *   **Problem:** The `TrustDnsResolver` swallows specific errors like timeouts and replaces them with a generic "No DNS records found" error. This violates our project rule to "Propagate Errors Explicitly".
    *   **Impact:** We lose valuable diagnostic information, making it harder to debug DNS issues.

4.  **`enrichment.rs`: Missing GeoIP Country Data**
    *   **Problem:** The `AsnInfo` struct and the spec's example output include a `country_code`, but our `MaxmindAsnLookup` service can't provide it and uses a placeholder.
    *   **Impact:** The final alert data is incomplete and does not match the specification.

### Medium-Priority Issues (Refactoring & Cleanup)

5.  **`network.rs`: Code Duplication in Message Handling**
    *   **Problem:** The WebSocket message processing loop is duplicated in `run_with_connection` and `process_messages`.
    *   **Impact:** Increased maintenance burden.

6.  **`matching.rs`: Inconsistent Logging (`println!`)**
    *   **Problem:** The hot-reload logic uses `println!` instead of the `log` crate.
    *   **Impact:** Inconsistent log output and loss of structured logging benefits.

7.  **`matching.rs`: Confusing `PatternWatcher` API**
    *   **Problem:** The API for `PatternWatcher::new` takes a `HashMap` of paths to tags, but the tags are ignored and re-derived from the filenames, making the API misleading.
    *   **Impact:** Poor developer experience and potential for confusion.

8.  **`network.rs`: Hardcoded TLS Configuration**
    *   **Problem:** The logic to accept self-signed certificates is hardcoded based on the URL containing "127.0.0.1".
    *   **Impact:** Brittle configuration that is not explicit.
