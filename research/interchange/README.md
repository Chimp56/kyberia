# Neutral planning interchange proof

This directory is a small, dependency-free Phase 0 proof for `OSS-007`.  It
defines and exercises the Kyberia `openrfplan/1` neutral planning document
without importing a Deconflict runtime, source tree, data file, or Python
package.  The harness is intentionally outside the Cargo workspace.  A future
adapter may translate this format to another planner after an independently
reviewed mapping; neither side is a dependency of the other.

The checked-in [`openrfplan-v1.json`](fixtures/openrfplan-v1.json) fixture
covers the minimum planning vocabulary from plan §§10 and Appendix I:

- a right-handed coordinate frame, metre coordinates, floor elevation, and
  vector boundary/wall/opening geometry.  A frame's `orientation_deg` is the
  explicit extrinsic Z-Y-X convention `[yaw_z, pitch_y, roll_x]` in parent
  axes, equivalent to `Rz(yaw) * Ry(pitch) * Rx(roll)` for local-to-parent
  coordinates; the root frame has world axes and a child origin is in its
  parent frame;
- material catalog references with source provenance;
- an access point and radio placement with explicit band/channel/width/power
  units;
- channel constraints, zone demand weight, and a predictive planning RSSI
  requirement;
- a deterministic seed and source/license provenance; and
- an explicitly preserved namespaced extension and unsupported channel value.

The format has no observations, packet evidence, tiles, uncertainty results,
throughput/SINR scores, optimizer output, or regulatory legality claims.  A
requirement is a planning input and is never a computed result.  Unknown typed
values carry a status, reason, unit, and raw token; they are not replaced with
zero, a default channel, or a default power.

`interchange.py` accepts bounded UTF-8 JSON and emits canonical bytes with
sorted object keys, ID-keyed collection ordering, and one decimal number form:
finite integers/decimals have no exponent, redundant fractional zeroes are
removed, and every zero is `0`.  This rule makes `0`, `0.0`, and `1e-6` versus
`0.000001` equivalent across the wire contract.  Access-point `radio_ids` and
constraint `allowed_channels` are membership collections; they are sorted by
their typed key and duplicate members are rejected.  Geometry vertices and
axis order remain ordered.  It rejects duplicate keys, non-finite numbers,
non-canonical IDs/references, frame cycles, unknown schema versions,
contradictory units, malformed typed values, and invalid cross-object
membership.  The input reader reads at most 256 KiB plus one sentinel byte.
Whole-document nesting, node, object, array, string, geometry, extension, and
seed limits are fixed in the module.  The root world frame must use identity
orientation; child frames use the documented extrinsic Z-Y-X convention.  The
`canonicalize` command is a real import/export path:

```sh
python3 research/interchange/interchange.py validate \
  research/interchange/fixtures/openrfplan-v1.json
python3 research/interchange/interchange.py canonicalize \
  research/interchange/fixtures/openrfplan-v1.json \
  .trash/test-runs/openrfplan-export.json
python3 -m unittest tests.test_planning_interchange
```

The seed source is the original one-line fixture input
[`openrfplan-seed-v1.txt`](fixtures/openrfplan-seed-v1.txt), whose SHA-256 is
`d40e48c3968d23f03947c896f7755d0bb8e815f57ebb37933806a3313b8c51da`.  It is
licensed CC0-1.0 for this proof.  The implementation uses only the Python
standard library; it has no third-party or transitive package license scope.
The canonical V1 fixture SHA-256 is recorded by the validation document and
bound by the focused tests.

This proof establishes deterministic local import/export and validation.  It
does not establish an upstream Deconflict acceptance test, foreign schema
compatibility, a production planner, RF propagation, channel legality, or a
final Phase 0 release decision.  Those remain explicit follow-up gates.
