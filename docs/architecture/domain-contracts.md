# Canonical evidence contracts

`crates/domain` is the inward, side-effect-free boundary required by plan sections
3, 7.1–7.7, 10.5/10.9/10.12/10.16, 11.4–11.6, 14.2–14.5 and Appendix I.
Its only production dependency is Serde. It does not acquire measurements,
generate IDs, read clocks, access files, or understand an external application’s
schema. Platform adapters construct validated values; application commands admit
complete envelopes to immutable stores.

## Units and identity

All numeric quantity constructors and deserializers reject NaN and infinity.
Durations, throughput and distances are nonnegative; frequencies are positive;
probabilities lie in [0, 1], percentages in [0, 100]. Signed spatial coordinates
are `CoordinateMeters`, distinct from nonnegative `Meters` lengths and signed
`Pixels` image coordinates. A coordinate only acquires meaning with its frame ID.
Signed zero is canonicalized to positive zero.

`Dbm` is absolute logarithmic power referenced to 1 mW. `Db` is a ratio.
`Dbm::difference` returns a `Db`; `apply_gain` consumes a `Db`. Both reject
overflow. There is deliberately no addition of two dBm powers. The future RF
metrics crate owns linear-power aggregation. Explicit EIRP, conducted-power and
power-spectral-density wrappers further distinguish radio power contexts.
Conversion methods use `TryFrom` because scale conversion can overflow or
underflow outside a destination’s physical domain. Angles use right-handed
radians or degrees, with no implicit normalization.

IDs are distinct Rust types backed by nonzero 128-bit values, serialized as
32 lowercase hexadecimal digits. The application must generate
collision-resistant IDs, for example UUIDv7 bytes, and enforce uniqueness in
storage; the pure domain does not claim to establish global uniqueness.
`ContentHash` contains SHA-256 bytes and serializes as 64 lowercase hex digits.
It identifies content, not a path. Hash calculation and integrity verification
belong to outer storage. A MAC address is six-octet evidence and never the
canonical physical-device ID. SSIDs retain up to 32 arbitrary octets, including
empty and non-UTF8 values. `Text` permits 1–1024 UTF-8 bytes, excludes controls
and all-whitespace strings; it does not sanitize HTML or certify a version.

## Unknown and schema compatibility

Every optionally observable measurement uses `Evidence<T>`:

```json
{"state":"known","detail":-62.5}
```

```json
{"state":"unknown","detail":"not_observable"}
```

Omission, null, not measured, not advertised, not applicable, not observable,
redacted, not retained, failed test and outside support have distinct semantics.
Unknown has no conversion to a numeric value. `EvidenceClass` distinguishes
observed, calibrated, inferred, interpolated, extrapolated and simulated derived
products; scan/frame payloads contain reported evidence only.

`SchemaVersion::V1` serializes as the string `"1"` for project, capability and
calibration contracts. Observation envelopes independently use
`ObservationSchemaVersion::V2`, serialized as `"2"`; adding observation V2 does not
make other contracts accept V2. Missing required fields and unknown versions/enum
variants fail deserialization. Additive observation object members are
ignored and cannot affect existing semantics. A producer must change the schema
version for semantic additions, new enum variants or changed units; these must
never be hidden in optional fields. Future-version raw bytes can be retained by
outer importers without admitting them as validated canonical observations.

Observation V1 input remains readable. Its required textual upstream version is
validated and migrated into `Evidence::Known(Text)`. V2 requires an explicit
`Evidence<Text>` instead. A source such as a database import that lacks the
producing software version uses `Unknown(SourceDidNotProvide)`. The upstream data
format version in `source_schema_version` is separate and is never substituted
for the upstream software version. No other observation fields change in this
migration. Canonical `EnvelopeData` construction and observation serialization
support V2 only; only the versioned envelope decoder reads V1.

`DecodedObservation` deserializes the envelope and returns a decoder-generated
receipt containing input/output schemas, `kyberia-observation/2.0.0` and whether
migration occurred. `ObservationEnvelope` also supports convenient deserialization
when the caller does not need the receipt. Imports that persist transformed data
must keep the receipt with their original artifact reference and immutable bytes.
The decoder never performs file I/O or overwrites old bytes. Observation identity
is preserved; serialized byte checksums change when the schema changes. Receipts
establish which decoder ran, not source authenticity.

Version dispatch validates typed wire fields and checks their shape against the
explicit schema. V1 with an evidence object and V2 with bare text both fail, as do
null/missing versions, duplicate known fields, invalid text and future schemas.
There is no semantic fallback or downgrade writer. The implementation uses Serde
without a JSON value-tree dependency in the core. Resource limits at the outer
framing boundary remain necessary before decoding.

Point-survey receipt snapshots independently emit top-level schema `"2"` while
their capture configuration remains V1. Their decoder accepts the exact original
untagged V1 shape, converting each textual source version to known evidence and
revalidating the complete point state. Only absence of the historical top-level
tag selects that legacy decoder: explicit null, `"1"`, future tags and mixed record
shapes fail. `DecodedPointSurvey` exposes its own receipt and decoder version
`kyberia-point-snapshot/2.0.0`. Empty snapshots with no tag are necessarily treated
as legacy because the historical empty representation has no differing record
field; this does not bypass state or configuration validation.

See [ADR 0003](ADR/0003-versioned-observation-migration.md) for policy, alternatives,
fixture provenance and migration validation.

## Time and spatial uncertainty

UTC is signed nanoseconds since the POSIX Unix epoch; the representation is
limited to approximately 1677–2262 and does not encode leap seconds. UTC source,
precision, and uncertainty remain explicit. A monotonic timestamp holds an epoch
ID and unsigned nanoseconds. Restarting a source requires a fresh epoch ID.
`elapsed_since` rejects cross-epoch comparison and reversed time, while retaining
nanoseconds until the final conversion to a floating-point duration. Epoch IDs
must be unique across sources; a source ID alone is insufficient after reboot.

Clock models retain reference UTC/monotonic values, offset, drift, error and
method version. Offsets are signed seconds added to source UTC to estimate
reference UTC. This contract does not yet implement synchronization fitting or
distributed pose fusion.

Position covariance is a symmetric 3×3 matrix in square meters packed as
`[xx, xy, xz, yy, yz, zz]`. Construction uses diagonal-pivoted LDL elimination
after scaling by the greatest absolute entry to prevent overflow. Negative
variances and a zero variance paired with nonzero covariance fail. Residual
pivots/entries tolerate roundoff up to 64 machine epsilons in scaled units;
tolerance is never applied to squared principal minors or the determinant,
which could conceal a materially negative eigenvalue in a small subspace.
Singular positive-semidefinite covariance is valid. This numerical acceptance
tolerance is not sensor uncertainty and does not establish calibration.

Cartesian metric frames are right-handed with +z up. Orientation is intrinsic
Z-Y-X yaw/pitch/roll with the right-hand rule. Pose references retain frame,
assignment revision, method, covariance and optional orientation. Editing a path
must create a new assignment, preserving original observations and timestamps.
Frame graph transformations, georeferencing, quaternion conversion, orientation
covariance and six-degree-of-freedom fusion are subsequent spatial work; the
present contract does not claim those capabilities.

## Envelope admission

`EnvelopeData` is a serializable staging record. Only
`ObservationEnvelope::new(data)` or deserialization to `ObservationEnvelope`
performs cross-field admission checks; stores must not accept `EnvelopeData`
directly. The envelope is immutable through its API (`data()` yields a shared
reference; `into_data()` consumes the envelope and requires fresh validation
after any edit).

The envelope retains observation/session/source identity, physical sensor and
adapter where known, source/adapter/parser/schema/driver/OS versions, capture
time, revisable pose reference, reported channel and tuned dwell context, privacy
state, quality flags, raw content reference, and typed scan/frame/health payload.
Calibration references do not mean a correction was applied: reported RSSI
stays raw. Outside-calibration-range state is explicit. Noise is unknown when
unobservable. Future calibration output must be a separate derived artifact.

Admission rejects mismatched clock model epochs, capture times outside a known
dwell window, duplicate chain indices, more than 16 chains or 32 quality flags,
invalid frame type/subtype values, and synthetic sources without a synthetic
quality flag. Dwell windows independently reject reversed or cross-epoch bounds.
Dwell can describe another channel from an advertised BSS’s operating channel;
these are intentionally separate. Nominal channel claims are retained as source
evidence; region, band/center/width/puncturing consistency belongs to the Wi-Fi
normalizer, not inferred by this container.

Privacy state records identifier handling and payload disposition. Retention
requires an authorization reference and deadline, but this pure contract does
not authenticate consent or perform pseudonymization. Application/privacy
services must enforce these policies before persistence. Artifact references
may be metadata records even when packet payloads were discarded.

The caller must bound serialized input size and nesting before deserialization.
The envelope’s admission limits prevent oversized canonical collections, but
Serde may allocate staging strings/vectors first. IPC/import adapters own input
framing, resource quotas, authentication, replay protection and error remediation.

## Capability negotiation

Capability documents retain collector identity/version, probe time, typed
capabilities, supporting evidence or conditions, and raw payload policy.
An absent capability is unknown. Conditional availability does not satisfy
`require_available`. Capability claims are evidence, not authorization tokens;
permission checks remain in platform adapters. The domain contains no table
assuming an operating system always supports a measurement.

## Validation and current scope

### Project, floors and calibration increment

The `project` module owns a canonical project→site→building→floor→map hierarchy.
Buildings own metric building frames. Floors own distinct metric floor frames,
a parent building-frame reference, a signed three-dimensional origin and yaw,
and a strictly positive clear height. Negative floor elevations represent
basements. `Floor::to_building` and `from_building` explicitly check the supplied
frame and apply a rigid yaw/translation transform; outputs retain meter types.
No external geometry, image-decoder or database object is stored in this graph.

A map asset has independent identity, floor attachment, pixel frame, positive
image dimensions, immutable content hash/media type/length, and provenance.
Multiple maps can reference identical source bytes without becoming the same
map. File existence, hash verification, malicious image decoding and resource
limits remain storage/import responsibilities.

`TwoPointCalibration` stores both image control points, target metric origin,
known distance, target segment direction, image y-axis handedness, and distance
and control-point uncertainty. It computes a positive `MetersPerPixel` scale
and invertible similarity transform. Typical raster +y-down is reflected before
rotation into the right-handed floor frame; +y-up images are explicitly
supported. `to_floor`/`to_image` require the correct source frame. Degenerate
controls, zero distance, frame loops, negative uncertainty and nonfinite
arithmetic fail. Map-calibration admission additionally checks frame ownership
and that controls lie within the continuous image rectangle [0,width]×[0,height].
Exact two-control-point fit does not establish measurement accuracy. Unknown
uncertainty remains unknown; multi-point least squares and covariance propagation
are not part of this two-point solver.

`Project::execute` consumes a v1 command request and returns a new immutable
project plus an operation receipt. Requests contain stable operation/project/
actor/device identity, expected base revision, explicit logical time and UTC
evidence. Reusing an operation ID, supplying another project ID, stale revision
or nonincreasing logical time fails without changing the original state. Actor
authentication and authorization are outer application responsibilities.

Commands create/remove sites, buildings and floors, import/remove maps, register
calibration revisions, activate an earlier calibration, and bind floor evidence.
Parent relationships, frame uniqueness, map/calibration references and positive
geometry constraints are rechecked on execution and deserialization. Maps and
frames cannot be changed once a floor is evidence-bound: an explicit future
coordinate-migration workflow is required. The binding is an immutable evidence
reference and cannot be undone by a simple command. Acquisition must bind a
floor in the same application transaction that first attaches spatial evidence;
the domain cannot detect undisclosed sensor data in another store.

Calibration undo changes the active calibration reference and retains all prior
calibration records. Receipt inverse commands run through normal validation;
they can be rejected when newer dependent objects or evidence make the operation
unsafe. Removing a map with historical calibration dependencies is deliberately
rejected, preventing orphaned history. Broader editor undo, archive/tombstone
semantics and explicit history-preserving map migration remain subsequent work.
Specifically, import→calibrate→undo calibration→undo import is not yet supported:
the retained calibration blocks map deletion with `HasDependents`, even after
its active reference is restored to unknown. A regression test pins this honest
limitation. Do not label this initial operation log as complete editor undo.

Receipts record exact accepted request, resulting revision, typed event and
inverse command or explicit nonapplicability. A deserialized receipt is untrusted
until `Project::replay` executes its request against the correct prior project
and compares the complete expected receipt. Forged events, inverses or revision
claims fail replay. This supports ordered deterministic replay; divergent offline
operation branches currently produce revision conflicts, with merge policy to
be implemented separately.

The snapshot uses deterministically ordered ID maps. Duplicate JSON object keys
are rejected instead of silently keeping the last object. Snapshots enforce
referential integrity and operation-revision index consistency; they are not
cryptographic proof of history. Content checksums/signatures and full replay are
separate storage/application verification steps. Current admission limits are
10,000 total hierarchy/calibration entities and 100,000 operations. This pure
metadata aggregate clones state per command; it must never receive individual
radio observations. A persistent structural-sharing implementation may replace
the internal representation after representative benchmarks.

Metadata baseline, 2026-09-07, local macOS ARM64, Rust 1.98.1, isolated workspace
default release profile: `cargo run --manifest-path crates/domain/Cargo.toml
--offline --release --example project_benchmark` created 100 sites/operations in
0.425 ms, 1,000 in 23.688 ms, and 10,000 in 2,416.783 ms. Inputs use sequential
explicit IDs, identical names, one project and no I/O or stochastic model. These
are descriptive single-run timings, not regression thresholds or a large mixed
building benchmark. An earlier 1,000-operation run took 48.132 ms while other
checks ran. The roughly quadratic total cost is visible: repeated cloning and
whole-state validation must be replaced or backgrounded before large-history
interactive replay. High-volume observations remain outside this aggregate.

Requirement links: §6 MAP-001 and MAP-003, §7.2, §10.6–10.7, §11.3/11.6,
§14.4–14.5, §16.3, backlog §18.1 FND-009/FND-011 and §18.4 MAPB-002/MAPB-008,
and §19 iteration 3. This increment supplies domain workflows only: calibrated
image import UI, operation persistence, migration/merge, geospatial frame graph,
materials/geometry editor and AP/radio identity graph remain open.

Run `cargo test --manifest-path crates/domain/Cargo.toml` and
`cargo clippy --manifest-path crates/domain/Cargo.toml --all-targets -- -D warnings`.
Compile-fail examples protect unit/identity/time separation. Property tests cover
unit round trips, Gram-matrix covariance, IDs and arbitrary byte decoding.
Adversarial cases test nonfinite deserialization, covariance determinants,
overflow, clock restart/reversal, schema drift, metadata bounds, unknown noise,
non-UTF8 SSIDs, duplicate chains and conditional capability rejection.

These increments implement foundational values, scan/frame/health contracts,
project hierarchy, two-point calibration and a bounded command/operation model,
not the complete domain graph. Active, spectrum, GPS and controller payloads,
analysis jobs, full radio identity graph, RF calibration, metric registry and
broader migration policies still require their own reviewed increments. No hardware
availability or RF accuracy claim follows from these contract tests.
