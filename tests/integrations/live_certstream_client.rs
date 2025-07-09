//! Live integration test for the CertStream client.
//!
//! This test requires a local certstream server running at wss://127.0.0.1:8181
//! and is enabled with the `live-tests` feature flag.
//!
//! To run this test:
//! `cargo test --test live_certstream_client --features live-tests -- --nocapture`

#![cfg(feature = "live-tests")]




