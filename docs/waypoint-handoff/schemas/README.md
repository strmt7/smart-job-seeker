# Contract status

These are design-time JSON Schema contracts and fictional fixtures, not an implemented API. They deliberately use example.invalid IDs and synthetic hashes. Structural validation cannot check semantic truth, digest correctness, network permission, expiry relative to a clock, cross-record links or one-use behavior. Those invariants must be enforced transactionally in Rust and tested under race/crash conditions.

Separate prefill approval from submit approval. The browser may transmit entered fields before a final Submit click; any field insertion into a live page therefore belongs to the disclosure boundary and requires an explicit prefill approval. A submit grant never retrospectively authorizes an earlier disclosure. An external write started flag means the irreversible operation may have begun, not that delivery is proven.

Receipt qualification must validate trusted origin, expected flow and application identity; a receipt ID in JSON alone is not sufficient. External user attestation is not an employer acknowledgement. A content hash is an integrity mechanism, not proof of source truth.
