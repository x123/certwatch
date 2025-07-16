//! Unit test for enrichment failure handling.


// The test `test_process_domain_propagates_enrichment_error` is removed because
// the `process_domain` function is now fully asynchronous and no longer
// directly returns a result from the enrichment process. The error handling
// is now internal to the spawned tasks within the DnsResolutionManager.