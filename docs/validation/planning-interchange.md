# Phase 0 neutral planning interchange proof

This is the bounded `OSS-007` proof for plan §§10, 16.13, 17, and Appendix I.
It validates the independent `openrfplan/1` canonical JSON harness in
`research/interchange/`.  The proof has no Deconflict runtime, source, vendor
data, or third-party Python dependency.  The fixture is original seeded input,
not an imported foreign project.

## Scope and contract

The fixture covers explicit metre coordinates in a right-handed x/y/z frame.
V1 defines `orientation_deg` as extrinsic Z-Y-X `[yaw_z, pitch_y, roll_x]` in
parent axes, with local-to-parent matrix `Rz(yaw) * Ry(pitch) * Rx(roll)`;
child origins are parent-frame coordinates and the root frame uses world axes.
It covers floor and vector geometry, material catalog references, access-point/radio
placements, typed band/channel/width/power settings, channel constraints, zone
demand weight, and a planning RSSI requirement.  It includes an unsupported
band token, an unsupported channel token, and a namespaced extension to prove
that unknown information remains visible.  Typed unknowns retain unit, reason,
and raw token.  The document contains no measurements, packet evidence,
uncertainty tiles, propagation score, optimizer result, or regulatory claim.

The reader bounds raw input at 256 KiB, nesting at 16, JSON values at 20,000,
object keys at 128, arrays at 4,096, strings at 4,096 UTF-8 bytes, geometry
vertices at 4,096, and seeds at unsigned 64-bit values.  It preflights UTF-8,
container depth, duplicate keys, integer token length, and finite numbers,
then validates exact fields, units, IDs, references, frame cycles, collection
membership, and cross-field shape.  Canonical output is compact UTF-8 JSON
with sorted keys, ID-keyed collections sorted by ID, and SHA-256 over those
bytes.  Numeric values use exponent-free decimal form with trailing fractional
zeroes removed and every zero emitted as `0`; access-point radio membership
and allowed-channel membership are sorted and duplicate members rejected.
Decimal exponents are bounded to 1,024 and decimal coefficients to 4,096
digits before canonical formatting.
The root world frame is required to have identity orientation, while child
frames use extrinsic Z-Y-X `[yaw_z, pitch_y, roll_x]` in parent axes.

## Reproduction

Run from the repository root with the Python standard library:

```sh
python3 research/interchange/interchange.py validate \
  research/interchange/fixtures/openrfplan-v1.json
python3 research/interchange/interchange.py canonicalize \
  research/interchange/fixtures/openrfplan-v1.json \
  .trash/test-runs/openrfplan-export.json
python3 -m unittest tests.test_planning_interchange
PYTHONPYCACHEPREFIX=.trash/test-runs/pycache python3 -m py_compile \
  research/interchange/interchange.py tests/test_planning_interchange.py
```

The focused test result on 2026-09-09 was **13 passed, 0 failed** under the
repository Python 3.9.6 interpreter.  The fixture is 3,017 bytes and its
canonical SHA-256 is
`c03c223e49f867e86446c1cbec08fcd50b427ca73c260cd649ec6556524ba580`.  The
original one-line seed source is
`research/interchange/fixtures/openrfplan-seed-v1.txt` with SHA-256
`d40e48c3968d23f03947c896f7755d0bb8e815f57ebb37933806a3313b8c51da`.

## Evidence limits

This is deterministic local import/export and hostile-input evidence only.  It
does not prove foreign schema compatibility, an upstream Deconflict test or
E2E, a production planner, an RF computation, channel legality, CAD/BIM/CRS
support, or Phase 0 completion.  A future mapping RFC, foreign-side fixture,
runtime/license review, and product integration tests remain open.

The output byte limit applies to the serialized artifact. The current bounded
serializer can transiently allocate more than that limit before rejecting an
expanded decimal document (independent review measured approximately 4.2 MB
from a 31,608-byte input). Input, node, coefficient and exponent limits still
bound the workload; a streaming output writer remains a recorded research
optimization. This is not a claim that peak memory is 256 KiB.
