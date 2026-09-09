# Reviewed publication integration checkpoint

The candidate and preflight correction are independently reviewed. Current-main
integration passes 615 workspace tests (nine ignored) and two separately reviewed
file-growth/truncation tests. See [correction review](../reviews/materialized-publication-correction-review.md).
Earlier author checkpoints below are historical; open publication budget findings
there are superseded by that review. Product wiring and full Windows validation
remain open.

# Materialized publication draft validation

Status: IN_PROGRESS; budget consumer implemented in the uncommitted isolated
implementation, not approved for integration.
Requirement: plan §§10.5–10.8, FND-011, and the downstream storage acceptance in
[project-materialization-packet.md](project-materialization-packet.md).

The storage draft now exposes explicit immutable baseline registration before
publication. Registration and publication use immediate SQLite transactions;
operation, aggregate, and bundle revisions remain separate. An exact historical
publication retry is checked before comparing the latest operation revision.
Read paths validate artifact registrations, canonical identities, historical
operation prefixes, protocol versions, and the current publication pointer.
This does not establish authenticated authorship of imported project evidence.

Executable evidence in `crates/project-store/tests/materialized_publication.rs`:

- Empty-operation publication, read-only reopen, and exact retry.
- Same-project baseline substitution preserves committed project and manifest.
- Two handles observe immutable registration before any publication; read-only
  registration fails and exact registration retries do not change the manifest.
- Registration retries reject missing artifact inventory. This test reproduced
  an accepted corrupt registration before the validation fix.
- An exact publication retry after another handle appends preserves the original
  receipt; stale fresh publication fails and the correct revision persists.
- Forged operation identity and publication ID, unsupported protocol/result
  versions, incorrect causal depth, and a current revision beyond the manifest
  fail reads and verification. Protocol version zero was accepted before the fix.
- Standalone baseline verification rejects altered counters, wrong project IDs,
  and duplicate registrations before any publication exists.
- Legacy absent-schema reads return explicit missing state; write migration
  creates empty tables without inventing baseline evidence.
- Corrupted baseline and result bytes fail reads, retries and verification,
  including after reopen, without changing the committed manifest.

Publication imports and schema constraints now use the materializer's 8,192
operation bound. Historical publication artifact validation checks the committed
manifest revision first. The obsolete implicit-registration branch was removed;
its original source is retained under the worktree's ignored `.trash/` directory.

The module's fault test injects failure after JSON projection but before SQLite
commit, then reopens the bundle. Committed state is unchanged, the stale projection
is detected, explicit recovery succeeds, and publication can be retried.
`schema_guard.rs` tests every nonempty incomplete subset of the three publication
tables in read-only and read-write modes.

Remaining required work before integration:

- Test baseline registration recovery boundaries and historical metadata tampering
  after a newer publication replaces the current pointer.
- Assess whether read validation sufficiently binds result content to replayed
  input rather than only to a self-consistent stored checksum.
- Add baseline read/access contract and durable ADR, update architecture/source
  inventories and traceability, and obtain independent review of the final diff.

Tests here exercise storage APIs, not product UI acceptance or complete FND-011.

Latest focused checks: `cargo test -p kyberia-project-store --test
materialized_publication --locked --offline` passes nine tests, and `cargo clippy
-p kyberia-project-store --all-targets --locked --offline -- -D warnings` passes.
The latest full storage regression passes 114 tests with one ignored test,
including the baseline verification and legacy fixes. Its retained log is
`.trash/storage-draft-regression-5.log` in this worktree. Independent review of
the current source has been requested; no approval is implied by these tests.

Independent review checkpoint: REQUEST_CHANGES. Reviewer Laplace independently
ran nine publication tests, fourteen schema-guard tests, Clippy, formatting and
whitespace checks successfully. Two MAJOR findings remain: result bytes can be
semantically substituted with consistent checksums without replay comparison;
whole-history verification repeatedly validates the complete operation inventory
and has no aggregate Rust-side work budget. A MINOR finding identified overly
broad state-table UPDATE authorization; the draft now lists only the four intended
mutable columns. No integration is authorized by this review. Add regressions for
semantic result substitution and aggregate verification work before resubmission.

Review remediation in progress: result validation now materializes the verified
baseline and historical operation prefix and compares the complete project. A
regression consistently rewrites the result file, checksum, publication row and
manifest to substitute a different valid project name; it failed before the fix
and now passes. Identity checks and replay share one prefix. Whole-history
verification loads one validated operation inventory instead of decoding and
validating that entire inventory for every publication. Prefix construction and
causal materialization still run per publication, so the aggregate-work MAJOR is
not yet closed. An independent design check has been requested for shared replay
budgets covering graph visits, conflict work and cumulative copy estimates.

Subsequent acceptance coverage: baseline registration has its own projection-before-
commit fault test, proving reopen retains unregistered state and explicit recovery
allows retry. `materialization_baseline` now loads canonical starting state through
the same bounded registration validation as verification; second-handle and
read-only reopen coverage includes the interval before first publication. Historical
revision tampering is rejected by whole-history verification and duplicate retry
after a newer result becomes current. Replay resource limits retain their typed
resource classification instead of being mislabeled as data corruption.

Independent follow-up confirms the semantic replay comparison is correct and the
shared inventory removes repeated SQL decoding. Aggregate replay/copy/graph work
remains MAJOR. Its pure-core prerequisite is assigned separately in
`.worktrees/shared-materialization-budget`; this storage worktree will consume
that reviewed API and charge outer decoding/prefix work before integration.

## Historical current-pointer regression

An adversarial test rewrites the current pointer to the valid revision-zero
publication after revision one has been published. Before the correction,
`verify()` incorrectly reported no failures. Full history verification now
rejects a current operation revision lower than any retained publication. The
test restores the latest pointer and verifies success before continuing the
existing historical-row corruption check. This does not prohibit legitimate
exact retries of older publications, which must leave the current pointer intact.
Focused publication tests pass (11); independent follow-up review is required.
This detects inconsistency within retained history, not rollback of an entire
bundle or authenticity of the storage medium.

After the current-pointer correction, `cargo test -p kyberia-project-store
--locked --offline` passes 117 tests with zero failures and one explicitly
ignored throughput benchmark. This includes all schema, observation, operation,
survey, bundle and publication suites. Clippy with warnings denied also passes.
The full-workspace 538-test result predates this correction; it is not represented
as a fresh full-workspace run.

## Identity error review and regression

Independent scoped review approved the identity-error correction after two
persisted-operation validation call sites were changed from the input mapper to
the persisted-data mapper. Input failures remain `Identity`, persisted failures
remain `Corrupt`, and direct/nested resource exhaustion remains `ResourceLimit`.
The decoder regression and category tests pass. A fresh full project-store run
using `cargo test -p kyberia-project-store --locked --offline` passes 119 tests,
zero failures, with one throughput benchmark ignored. The reviewed correction
does not close the cumulative-budget finding.

## Cumulative verification budget

The publication verifier now accepts a caller-owned
`ResourceBudget<CancellationHook>` and retains the existing no-argument wrapper.
The same budget covers manifest and baseline decoding, operation inventory
validation, every historical prefix, identity encoding, materializer replay, and
all immutable result checks in deterministic publication-ID order. A verified
baseline and one decoded operation inventory are reused for the transaction;
historical prefixes still undergo their own causal validation and consume the
shared counters.

The storage boundary precharges known metadata/BLOB/project-copy proxies before
owned values are decoded or cloned. Every publication, baseline, state and
operation metadata path first reads scalar SQLite text lengths, rejects values
outside the fixed field contract, and charges the admitted text bytes before a
Rust `String` is constructed. This applies to historical inventory and to
lookup/retry paths as well as the publication verifier. Baseline identity
decoding is charged for the domain decode, canonical JSON, retained project
bytes and identity encoding; materialized project readback is charged for
domain decode and canonical JSON. Operation rows are admitted from scalar
revision/text/BLOB length preflights before BLOB loading and operation decoding.
These are deterministic cumulative proxies, not claims about exact resident
heap.

Artifact reads open the final file component with platform-specific no-follow
flags, inspect that same handle's size, require the declared manifest length
before allocation, and read through the handle with a one-byte overrun guard.
This keeps a replaced or raced artifact from bypassing the declared-byte budget;
unsupported platforms fail explicitly rather than silently weakening the
path-safety contract.

The generic `Bundle::verify` inventory scan retains its existing per-artifact
bound and is outside the caller-owned cumulative materialized-publication
verification budget; this packet does not claim a whole-bundle aggregate
artifact-byte cap.

`StoreError::Cancelled` is preserved for direct and nested identity, operation,
merge and materializer cancellation. Resource exhaustion remains an explicit
`PublicationError::ResourceLimit`; no later publication is skipped or reported as
verified after exhaustion. The focused publication suite passes 13 tests,
including valid multi-publication cumulative exhaustion, deterministic repeated
failure, cancellation after replay copy work begins, empty-budget preflight, and
the decoder-stage quota boundary. Independent review and main integration remain
open.
