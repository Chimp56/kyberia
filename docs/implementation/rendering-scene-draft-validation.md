# Renderer-neutral scene draft validation

Status: independently approved bounded adapter through `39f6555`; integrated workspace
regression passes 585 tests with zero failures and nine ignored tests. No renderer selection or product capability is approved
by this document. Relevant requirements: Gate B,
UX-001–004/008/009 and ADR 0008.

`kyberia-rendering-scene` projects canonical spatial-analysis tiles into a
versioned, canonical JSON scene with numeric inspection data. It preserves floor
and frame IDs, grid offsets and cell centers, observed/estimated/unknown classes,
unknown reasons, measurement group references and metric/source provenance.
Presentation colors are separate from the canonical scene identity.

Current tests cover canonical round trips, input permutations, source checksum
mismatch, schema rejection, geometry/frame contradictions, unknown preservation,
duplicate contributor rejection, and observed values agreeing with their cited
measurement group. The observed-value tests exercise both mutable Tile admission
and serialized scene admission; both paths previously accepted contradictions.
The positive fixture samples the actual offset cell center (7.5, 11.5), rather
than claiming observation support at an unrelated cell.
Tile admission also rejects group aggregation methods that disagree with the
configured metric, even when a single sample produces the same numeric value.
The serialized scene carries canonical metric-definition bytes and a versioned
artifact reference. Import verifies their binding, metric identity and aggregation
selection, and rejects group-method substitutions. Interpolation configuration
survives round trip and must agree with the definition's spatial method. Tests
tamper definition bytes, hashes, aggregation and configuration separately.
This binds the metric definition and the complete numerical tile computation
when the scene is admitted through either validated path.
Canonical encoding uses a bounded writer; exact-limit and one-byte-overflow
tests cover escaped strings. Shape admission supplies the separate deterministic
working-set and evaluation bounds described below.

Serialized duplicate-contributor coverage ensures repeated group references do not
count as independent support. Extrapolated cells are rejected when the preserved
configuration disables extrapolation; this contradiction was accepted before the
regression fix. Independent review returned REQUEST_CHANGES for numerical binding,
geometric attribution and allocation limits. These findings remain the integration
criteria; the changes below have not yet received follow-up approval.

Direct Tile admission now reconstructs the canonical Model and compares replayed
cells, location groups and canonical sample order. Regression tests reject forged
interpolated values, shifted observed-grid origins and excessive replay work. A
counting serializer admits the encoded input size before cloning. Shape admission
also rejects more than `MAX_SCENE_DISTANCE_EVALUATIONS` (currently the spatial
analysis 100-million evaluation cap) and more than the 256 MiB deterministic
working-set budget before model or projection clones. The estimate counts the
observed capacities of the input/decoded vectors and derives element terms from
`size_of`: four overlapping sample collections, four group collections, three cell
collections, both observation-ID vectors per group, and contributor vectors per
cell. It also reserves the known-sample pointer vector, one aggregate's
`StaticSignalSample` plus two possible `f64` scratch vectors, BTree aggregation
entries, the structural-validation sample map and ID sets, bounded neighbor
heap/weights, fixed Model/Tile/SceneWire/SceneDocument values, and the bounded
metric definition bytes. A four-pointer-word pad is charged per live Vec or tree
allocation for capacity/bookkeeping accounting. This is an explicit admission
policy, not a guarantee about allocator RSS; `size_of` may produce different
admission thresholds on different targets, while it is excluded from canonical
scene bytes and identity.
Serialized scenes now preserve canonical samples, including position covariance.
Import reconstructs the Model and compares the entire projected wire with the
replayed tile. Regression mutations cover derived values, grid origin and sample
values; each fails numerical validation before canonical encoding checks. Both
admission paths poll cancellation during structural scans, projection, bounded
encoding and hashing; serialized decoding polls every 4 KiB. Cancellation returns
no scene after any of those stages. This proves internal computational consistency,
not source authenticity or the correctness of the upstream source decoder.

The scene intentionally retains canonical sample IDs, positions, values and
position covariance so a renderer can replay and inspect evidence. It carries no
raw BSSID or source payload. The source artifact reference remains a provenance
pointer: `from_verified_tile` checks supplied bytes against its hash, but does not
prove that a source decoder produced the preserved samples. Callers must apply
their project retention policy before publishing a scene. This adapter's hard
working-set and evaluation caps are fail-closed; larger coverage must be tiled.

Validation: `cargo test -p kyberia-rendering-scene --locked --offline` passes thirty-five
tests (fourteen unit tests and twenty-one integration tests). `cargo clippy -p
kyberia-rendering-scene --all-targets --locked --offline -- -D warnings` passes.
Formatting and whitespace checks pass.

The representative resource fixture builds a valid 512-sample, 32x32 tile and
successfully projects all 1,024 cells. The same tile cancels after exactly 32
cooperative admission polls and returns no scene. The
`resource_measurement_harness_covers_direct_import_and_cancelled_paths` test
also measures direct projection, canonical import, and cancellation on that
fixture and prints a reproducible accounting record. One current host run
reported `encoded_bytes=618346`, `working_set_estimate=70080530`, direct
projection `95150us`, import `124806us`, and cancellation `9us` after eight
preflight polls. The byte estimate is deterministic for the target's type
layouts; elapsed values are diagnostic observations, not product SLOs, and the
test does not claim allocator RSS. The hostile nested-array regression also
processed a 200,045-byte input and rejected it during preflight in 9,378us on
that run; this is parser-work evidence, not a general throughput guarantee.

Canonical import now performs a streaming, non-retaining shape preflight
before `serde_json` constructs `SceneWire`. It counts samples, groups, both
observation-ID arrays, cells, contributors, and metric-definition bytes,
rejecting their explicit limits and a 64-level JSON nesting bound. The
preflight uses a two-times count capacity allowance for geometric `Vec` growth
and feeds the existing working-set estimate before typed decoding. It polls
cancellation while traversing the input. A hostile nested observation-ID array
exceeding `MAX_SAMPLES` is rejected with
`ResourceLimit("group observation ids")` before typed decode; strict typed
deserialization, canonical replay, and post-decode capacity checks remain in
force for valid inputs. Duplicate known fields are rejected before their
duplicate values are traversed, including repeated root arrays and nested group,
aggregate, and cell arrays. Serde may still allocate temporary key or escaped
string scratch while parsing the encoded input; those values are not retained,
and the encoded-byte cap plus non-retained counted arrays bound parser work.
This is not an allocator-RSS guarantee.

Remaining product and platform requirements (bounded adapter review is approved):

- Independently review the complete numerical replay on both admission paths.
  Source checksum agreement alone does not authenticate the samples.
- Measure real allocation/RSS and cancellation responsiveness on additional
  platform-representative large tiles; the deterministic caps and current host
  elapsed diagnostics are not Gate B performance evidence. Target-dependent
  `size_of` accounting must be reported per target and never used in canonical
  identity.
- Follow-up independent review approved `39f6555` with 35 passing tests and no
  BLOCKER or MAJOR findings. The architecture inventory now classifies
  this crate as an outward adapter, and the dependency inventory records the
  changed lockfile hash without adding external dependencies.
- Wire real canonical tiles into the comparative renderer harness, then complete
  the platform, accessibility, recovery and performance gates in ADR 0008.

This adapter is not a usable survey UI or a completed renderer gate. Its current
structural checks are not a substitute for validating numerical computation.

Follow-up review accepted direct and serialized numerical replay, but found that
direct admission still copied caller sample ordering. Projection now uses the
replayed canonical tile; the permutation test reverses a public Tile's samples
and checks identical bytes, hash and successful reopen. Serialized import now
has a cancellation-aware entry point, polling between stages and inside spatial
replay. Structural validation, projection, encoding and hashing use the same
latched cancellation outcome, and resource shape admission runs before clones.
JSON import now adds a streaming shape/depth preflight before typed
deserialization, while the deterministic working-set estimate and current host
measurement remain allocation proxies. Allocator RSS, cross-target expansion
validation, and full Gate B performance evidence remain open.
