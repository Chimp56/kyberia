# ADR-0009: Bind spatial analysis identity to immutable artifacts and canonical JSON

Status: Accepted after independent architecture/security review. Job execution, metric registry and cache storage remain open.

## Context

Plan §§3, 10.8, 11.7–11.8 and backlog FND-008 require reproducible analyses and hashes of canonical job specifications plus immutable inputs. Mutable project versions or a random seed alone do not identify the evidence, computation or numerical environment. The existing domain has content hashes, unit-safe quantities and opaque IDs; it must stay independent of loaders, storage and solver runtimes.

## Decision

Introduce a validated immutable spatial-analysis manifest with explicit content references for every input/policy/model/code/execution-profile dependency, typed scalar/artifact parameters, explicit randomness and a framed grid. Canonicalize only declared sets; preserve semantic sequences inside their pinned artifact. Use full-width decimal-string integers and finite, normalized unit values. Hash the complete versioned compact JSON representation with existing pinned SHA-256 and serde libraries.

The V1 encoder is an explicitly documented format, not RFC 8785. Pin its byte representation through an independently constructed golden and property tests. Enable serde_json `float_roundtrip`: an executable property test demonstrated a hash-changing parse under the default feature set. Preserve V1 encoding when later schemas or serializer versions are introduced. Authoritative input bytes must be verified through the outer artifact port before execution, including transitive snapshot/chunk references; a syntactically valid manifest is not proof that its dependencies exist or satisfy the metric contract.

## Alternatives

Hashing arbitrary user JSON makes field order/whitespace accidental cache inputs and hides parameter units. Hashing only project/algorithm version labels permits changed content to reuse a result. Browser JSON numbers cannot preserve full-width integer seeds. RFC 8785 with a compatible encoder is a possible future interoperable schema; this increment uses the already pinned serializer and a closed typed format, without implementing a separate number/string serializer. Including execution timestamps/run IDs prevents reuse of an identical computation; those belong in its execution record.

## Evidence

The [independent Luna xhigh review](../../reviews/manifest-luna-review.md) verified the frozen implementation, golden bytes, malformed-input probes, workspace regression and dependency boundaries. It found no BLOCKER or MAJOR implementation issue. Integration refreshes the unit-source digest and clarifies the historical enum regression; additional individual metadata-mutation cases remain documented test debt.

The [format and validation procedure](../analysis-manifest.md) documents exact encoding and limits. Tests reproduce and fix binary64 round-trip drift at `-2.5295671164902154e-213`, reject unknown fields in empty enum variants, preserve u64::MAX, compare independently encoded bytes/SHA-256, vary 20 cache inputs, reorder set collections, and reject invalid/precision-collapsed grids. Whole-workspace regression validates the additional serde feature across existing parsers. No new external package version is introduced.

## Consequences

Canonical identity is computable without clocks, filesystem or engine objects. Adapters must retain immutable dependency closure and consumers must validate referenced payload schemas. A content hash proves bytes, not scientific correctness or permission to execute an algorithm. The new domain dependencies provide pure serialization and hashing only. Metadata limits do not authorize a billion-cell allocation; execution imposes per-tile resource/cancellation budgets.

## Reversibility

The wire schema is separately versioned and unrelated to observation/project versions. V1 golden bytes remain supported; later schema versions require explicit migration/compatibility fixtures and new hashes. Adding another job type must define its own required inputs and must not weaken existing spatial input requirements.

## Validation plan

Require independent architecture/security review, golden and property tests, malformed-input/precision/seed/cache-invalidation checks, dependency-direction checks and workspace regressions. Before user-facing analysis completion, connect immutable artifact loading, metric compatibility, tiled execution, job status/cancellation, cache persistence and UI numerical inspection; test replay from a reopened project. This ADR does not defer those requirements.
