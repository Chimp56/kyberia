# ADR-0027: Neutral planning interchange proof

- Status: Accepted bounded Phase 0 proof; upstream interoperability and product mapping remain open
- Related: plan §§10, 16.13, 17, Appendix I; `OSS-007`; `execution-dag.json` node `interchange-proof`

## Context

RF Atlas owns canonical observations, analysis results, and application
identity.  Deconflict is a reference/contribution target and must not become a
runtime or data dependency.  Phase 0 still needs a reproducible interchange
proof for a small planning vocabulary so geometry, access-point/radio
placements, and channel constraints can cross a future adapter boundary.

The proof must remain useful when a foreign producer cannot supply a value.  A
missing or unsupported band, channel, or power value must remain visibly
unknown, with its unit and raw token, instead of becoming a fabricated numeric
default.  It also needs a stable identity independent of object insertion
order, bounded hostile input handling, and source/license provenance.

## Decision

Adopt the independent `openrfplan/1` canonical JSON proof under
`research/interchange/`.  Its fixed top-level envelope contains:

1. schema and plan identity;
2. metre, MHz, dBm, degree, and unitless declarations;
3. right-handed coordinate frames and floor elevations;
4. vector geometry with material catalog references;
5. access points, radios, and typed channel settings;
6. typed channel constraints, zone demand weights, and planning requirements;
7. deterministic seeds; and
8. source/license provenance plus bounded namespaced extensions.

The V1 parser rejects unknown top-level fields and future schema versions.  It
rejects duplicate JSON keys and non-finite numbers, validates all IDs and
cross-object references, rejects frame cycles and contradictory associations,
and enforces byte, depth, node, collection, string, coordinate, and seed
limits.  Canonical encoding sorts object keys and ID-keyed collections, then
uses SHA-256 over the exact UTF-8 bytes.  Numeric values use one bounded
exponent-free decimal grammar: trailing fractional zeroes are removed and
negative or positive zero emits as `0`, so equivalent JSON numeric spellings
have one identity.  Access-point `radio_ids` and constraint
`allowed_channels` are typed membership collections sorted by key with
duplicates rejected.  Decimal exponents are bounded to 1,024 and coefficients
to 4,096 digits before formatting; vertices and axis order retain semantic
order.  Typed known/unknown values preserve both the expected unit and an
unknown reason/raw token.

`orientation_deg` is not an undocumented Euler triple: V1 uses extrinsic
Z-Y-X rotations `[yaw_z, pitch_y, roll_x]` in parent axes, with local-to-parent
matrix `Rz(yaw) * Ry(pitch) * Rx(roll)`.  A root frame uses world axes and a
child origin is expressed in its parent frame.  The fixture uses the identity
rotation, but the convention is part of the wire contract for future nonzero
values.

The fixture and parser use only Python's standard library and an independently
authored seed.  No Deconflict source, runtime, vendor data, RF heuristic,
propagation score, observation, or result is admitted to the neutral minimum.

## Alternatives

1. **Import a Deconflict object model directly.** Rejected because it would
   couple the canonical application boundary to foreign runtime/source types
   and their licensing/runtime availability.
2. **Use untyped arbitrary JSON.** Rejected because units, unknown semantics,
   references, and hostile-input limits would be implicit and non-auditable.
3. **Put observations or RF scores in the shared document.** Rejected because
   those are RF Atlas-owned evidence/results and require different provenance,
   uncertainty, and version contracts.
4. **Require a live foreign runtime for the Phase 0 proof.** Rejected because
   the plan explicitly calls for deterministic round trips independently of
   the Deconflict runtime; upstream interoperability remains a later gate.

## Evidence

`tests/test_planning_interchange.py` independently checks the seeded fixture
hash, canonical bytes, collection permutation identity, CLI import/export,
unknown and extension preservation, duplicate keys, invalid numbers, future
versions, references, frame/unit/shape contradictions, raw byte/depth/node
limits, and absence of promoted observations/scores/foreign imports.  The
validation record reports the exact commands and local results; it does not
claim an upstream runtime or acceptance result.

## Consequences

The bridge can exchange a small planning document with deterministic bytes and
auditable unknowns while keeping all foreign adapters outside the domain.  The
fixed V1 minimum intentionally excludes richer CAD/BIM, CRS, 3-D material
attenuation, RF propagation, optimization, observations, and report outputs.
Namespaced extensions can preserve bounded foreign metadata but do not grant it
canonical semantics.  An adapter must validate its own mapping and provenance
before producing a V1 document.

## Reversibility and follow-up

The proof is isolated and can be replaced by a reviewed Rust/application
adapter while retaining the same version/hash decision.  A V2 change requires
an explicit migration or a separate decoder and new fixtures.  Before any
foreign interoperability claim, maintainers must agree on a mapping RFC and
run deterministic foreign-side fixtures, source/license review, and the
remaining Phase 0 and product gates.  This ADR does not select Deconflict as a
renderer, optimizer, RF engine, or production dependency.
