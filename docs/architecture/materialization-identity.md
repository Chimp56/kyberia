# Materialization input identity

`kyberia-materialization-identity` defines the content identities required
before a project materializer or storage adapter may combine a baseline with an
operation set. It is a pure inward crate and performs no replay, persistence,
or project mutation.

`BaselineIdentity` contains a versioned binary envelope with a domain-separated
magic value, project ID, domain project revision, domain logical time, and the
exact canonical JSON bytes emitted by `kyberia-domain::Project`. Decoding
re-parses the project, checks aggregate validation, checks the repeated identity
fields, and rejects any non-canonical or trailing bytes. Its SHA-256 digest is
computed over a separate identity domain plus the complete envelope.

Project canonicalization is bounded at the identity boundary. A serialization
that reaches the project-byte ceiling returns the structured
`ResourceLimit("project_baseline_bytes")` error; it is not relabeled as a
canonicality failure. The boundary test exercises exact-limit success and
one-byte-over-limit rejection without allocating a large project.

`OperationSetIdentity` contains the project ID, sorted operation membership,
each operation ID and content hash, and the complete canonical operation wire
bytes. Decoding validates every operation through
`kyberia-operation-log::Operation::from_bytes`, reconstructs the bounded DAG,
and compares the re-encoded envelope byte-for-byte. Input order therefore has
no effect, while membership, operation bytes, project identity, and graph
validity remain binding. The identity artifact is bounded to 64 MiB.

The operation entry retains the operation-log schema version and typed payload
verbatim. In particular, V2 unknown calibration evidence and V2 resolution
values round-trip through this identity without sentinel identifiers or a
second interpretation layer. V2 wire-byte and entry-hash tampering is rejected
before an identity is accepted; causal replay and conflict semantics remain
owned by the operation-log/materializer boundaries.

`MaterializationIdentity::bind` checks that both identities name the same
project and exposes the two independent content hashes together with the
baseline revision/logical-time metadata. It deliberately does not equate those
counters with `ProjectVersion`, operation causal depth, or the eventual
materialized revision. It also does not validate operation inverse claims;
that belongs to the reviewed materializer and domain application boundary.

The binary envelope is an application identity format rather than a domain
serialization replacement. Existing project and operation bytes remain
readable, and no V1 operation is rewritten.
