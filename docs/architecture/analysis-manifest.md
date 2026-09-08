# Spatial analysis manifest V1

`kyberia_domain::analysis::AnalysisManifest` is a pure immutable request identity for spatial metric jobs (plan §10.8, backlog FND-008). It computes canonical bytes and SHA-256, verifies supplied artifact bytes, and rejects malformed or ambiguous specifications. It does not execute jobs, read artifacts, schedule a graph, or implement a cache. Those application/storage integrations remain open.

The manifest pins survey session/snapshot identities and content, identity graph, geometry, metric definition, client profile when applicable, selection policy, algorithm code/build identity, execution profile, parameters, randomness, grid and area mask. The metric-definition artifact is the canonical bounded `MetricDefinition` from ADR-0017; its registry ID/revision and SHA-256 bind the same semantic unit, unknown policy, aggregation, and evidence requirements used by compute and presentation. Every external artifact includes a semantic version, SHA-256, exact byte length and media type. A mutable name/version alone cannot identify an input. Snapshot artifacts must include immutable observation/chunk references; loading and recursively verifying that closure belongs to the artifact adapter before execution. A hash establishes content identity, not trust, calibration or scientific validity. Historical
signal-metric-definition artifacts remain readable at the registry boundary with
their original media type, version label, bytes and hash preserved.

Parameters carry explicit unit tags. Dimensionless scalars are distinct from meters; compound arrays/models can be immutable typed artifacts. The selection policy artifact specifies filters, grouping, evidence and uncertainty semantics. An execution-profile artifact records CPU/backend/runtime identities and numerical tolerances relevant to reproducibility. Artifact payload schemas and compatibility with a metric registry must be validated by the consuming job; this increment does not invent missing policy/model schemas or claim that arbitrary pinned bytes are executable.

## Canonical bytes

The schema identifier is `kyberia-spatial-analysis/1`. V1 uses compact UTF-8 JSON with no trailing newline, struct field order defined by the versioned wire types, and enum tags exactly as serialized by pinned serde 1.0.228 / serde_json 1.0.149. It is **not RFC 8785**. Changes to those dependencies require golden compatibility checks; an encoding change requires a new canonical schema version while V1 remains readable/hashable.

Survey inputs form a set sorted by opaque session ID then snapshot ID. Multiple snapshots of one session are rejected. Parameters form a set sorted by name's Rust Unicode string order; duplicates are rejected. Execution-sensitive sequences belong in an artifact and are never sorted. Unicode is preserved without normalization; different code-point strings remain different identities.

All 64-bit seeds, counts and artifact lengths serialize as canonical unsigned decimal strings, with no signs/leading zeros. Grid dimensions are bounded u32 JSON numbers. Finite binary64 values use the pinned serializer's shortest representation; `float_roundtrip` is mandatory for decoding. Unit constructors normalize negative zero. A property test found the concrete value `-2.5295671164902154e-213` changed on default deserialization; that counterexample is retained. During development, unit variants for deterministic randomness and an inapplicable client profile accepted extra fields despite `deny_unknown_fields`. The current implementation uses empty struct variants; retained regressions verify rejection.

The digest is SHA-256 of the complete canonical JSON, including its schema identifier. Execution timestamps, progress and analysis-run IDs are intentionally outside this computation identity; they belong to a separate execution record. No randomness is inferred: the caller selects deterministic execution or supplies an exact seed. Claims of deterministic execution must still match the actual algorithm/execution profile.

The independently encoded original fixture `crates/domain/tests/fixtures/analysis-manifest-v1.json` has 2,183 bytes and SHA-256 `ae595b6518430dd7ffc8424aadb16678b0b2b076ed063397bc48d1c2c69539f0`. It was constructed using Python's ordered JSON encoder and independently hashed with hashlib, without invoking the Rust implementation. Its artifact hashes are intentionally synthetic; it is a format oracle, not a measured-data/job execution fixture.

## Limits and geometry

The decoder accepts at most 1 MiB, 4,096 survey inputs, and 128 scalar/artifact parameters. All struct/enum wire fields are closed; unknown/duplicate fields, malformed numeric values, future schemas and trailing content fail. Serde's default recursion limit remains enabled. Allocation during decoding is bounded by input bytes; construction callers already own their supplied vectors and artifacts. No project or RF data is synthesized.

The output grid uses a named floor and Cartesian meter frame, a lower-left outer corner, elevation, positive resolution and positive rows/columns. Cell centers are origin + (index + 0.5) × resolution in each axis. The grid is limited to one billion cells as a metadata envelope, not as permission to allocate a billion cells at once; tiled execution imposes its own stricter work/memory budgets. Extents must be finite and boundary/center positions must remain distinguishable in binary64. Resolution must exceed a conservative roundoff budget of eight binary64 epsilons times the largest absolute origin, endpoint or extent; this bounds multiplication/addition error without iterating a potentially enormous grid. The occupied/support mask is an explicit content reference. The executor must verify its frame, shape, policy and evidence support; a mask reference does not automatically turn cells into known measurements.

## Verification

Run `cargo test -p kyberia-domain --test analysis_manifest --locked --offline` and workspace regression checks. Tests mutate 20 major cache inputs, reorder sets and JSON fields, preserve full u64 seeds and arbitrary finite binary64 values, reject duplicate/future/oversized data, detect degenerate grids, and verify artifact bytes and lengths independently. The [independent review](../reviews/manifest-luna-review.md) approves the implementation and records narrower metadata-mutation coverage as test debt. The pure domain adds only existing pinned JSON and SHA-256 libraries; no adapter, storage, UI or external engine objects enter it.

The explicit release benchmark is `cargo test -p kyberia-domain --test analysis_manifest --release --locked --offline benchmark_manifest_roundtrip -- --ignored --nocapture`. On the shared macOS 26.6.2 ARM64 host with Rust 1.98.1, the original synthetic workload produced:

| Survey references | Canonical bytes | Encode/hash µs | Decode/validate/hash µs |
|---|---:|---:|---:|
| 1 | 1,929 | 331 | 208 |
| 100 | 27,075 | 1,258 | 1,281 |
| 1,000 | 255,675 | 8,011 | 7,936 |
| 4,096 | 1,042,059 | 21,176 | 20,012 |

Each case verifies complete round-trip equality. These are single-run baselines, not stable CI thresholds or UI-thread latency guarantees; dependency artifact I/O/verification and solver execution are excluded.
