# Materialization identity review

Disposition: APPROVED with one accepted MINOR follow-up.
Reviewer: `/root`, independent of the author.
Candidate: `073acda029266e35158aa698ac1a5403754e953e`.

Root independently ran `cargo test -p kyberia-materialization-identity --locked
--offline`: all 12 tests passed. Review covered domain-separated versioned
encoding, exact baseline state binding, operation-ID ordering and membership,
operation hash verification, project mismatch rejection, bounded decoding,
bounded serialization output and golden bytes/hashes. The implementation depends
only on inward domain/operation contracts and serialization/hash libraries.

MINOR: `canonical_project_bytes` maps a bounded writer's size-limit error to
`NonCanonicalEncoding`, even though `ResourceLimit` would communicate the cause
more accurately. The byte limit still fails closed; this does not bypass the
resource guard or change evidence. Correct the error classification in follow-up
with a serialization-limit test. The 64 MiB limit is an artifact byte limit,
not a claim that total temporary process memory stays below 64 MiB.

Integration must rerun identity tests against main's newer V2 operation contract
and broader regression suites. This approval covers exact input identity only:
it does not prove causal priors, undo validity, conflict resolution, aggregate
materialization or transactional publication. Those remain open under FND-011.
