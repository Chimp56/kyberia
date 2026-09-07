# Independent research harness review

Reviewer: `/root/qa_spec_audit`. Author: `/root/harness`.
Decision: **APPROVED for the selected research fixture/evidence-checker scope**.
No unresolved BLOCKER, MAJOR, MINOR, or NIT findings remain in this scope.

The reviewer read the source, tests, generated fixtures/catalog, source ledger,
and documentation in `/private/tmp/kyberia-harness`. Generated fixture/catalog
parity was independently tested. The working tree was based on preservation
commit `4e3bc5213e5a4b322ea56ef5253c24124f827f07`; its implementation was not yet
committed. The hashes below identify the actual final reviewed bytes, rather
than incorrectly attributing them to that baseline commit.

| Reviewed file | SHA-256 |
|---|---|
| `tools/validation/fixtures.py` | `5b545fd0931679d620b26315a3864748486472e86e0084d7a094a9d92030e039` |
| `tools/validation/runtime_gates.py` | `da964f8641b189612afb39e5e4d755950a9229e93483203ffd7ce2c3b037c280` |
| `tools/validation/generate_catalog.py` | `e1a77424308e236e570fe5c86c43cd9de4d902f16e5e3b6d4bccab5e87edf35b` |
| `tools/validation/gates.json` | `aa49e0347ffec24d97fd1e9d7111c447b069198b0b69d41d9730fef46f9561da` |
| `tests/test_research_harness.py` | `c2fc6e3d89f5832b2567d8a5ec0a9c24333a8f0627f4b6990a970d9d54f25949` |
| `fixtures/synthetic-scenes/canonical-v1.json` | `b960443f591d80e36c8ed98d8df14f232642cb386c4dc9a8333e165b86a3927a` |
| `fixtures/synthetic-scenes/survey-v1.json` | `a3188519f4a57687634ff02283e671f52e767df36cde3c15c2b26b5444c068cf` |
| `fixtures/wifiheatmap-oracle/tin-v1.json` | `708614f9ab64f2a05ed7ab4bea067aa126a2fbbf5310a4837b82c59ae07ee4a2` |
| `docs/validation/synthetic-fixtures.md` | `326073f0548aef58620eda636a0c91a4ae53ce7dde6b600afb981a116873d1bb` |
| `docs/validation/runtime-gates.md` | `bfff33c310de0486772511561d90c97eadfb4246d8b8c48acc185f5950b3a867` |
| `docs/licenses/fixture-sources.json` | `123c7c0d10a4c3c00cb35144980a58b0d0100d51b32ef26835b3bf95ebfd6809` |

## Corrected findings

1. **MINOR — antenna sign regression was ineffective.** The original cardinal
   yaw/+x receiver case used a symmetric cosine gain pattern and could not detect
   reversed yaw. The author added a 45-degree off-axis unit direction: yaw +45
   aligns the antenna and yields −57 dBm, while yaw −45 is orthogonal and yields
   −60 dBm. A negative comparison now explicitly detects reversed rotation.
2. **MINOR — overflowing JSON numbers bypassed nonfinite rejection.** The
   reviewer reproduced `{"value":1e999}` becoming positive infinity despite
   NaN/Infinity-literal rejection. A finite `parse_float` hook now rejects both
   signs of overflow. Independent retesting confirmed ±1e999 rejection and
   finite 1e100 acceptance.
3. **MINOR — synthetic covariance layout was implicit.** Four covariance entries
   appeared beside a three-dimensional position. The fixture and documentation
   now declare 2×2 XY row-major `[xx, xy, yx, yy]` covariance and synthetic fixed
   z=1.5 m, without claiming measured vertical precision.

The author's earlier whitespace-placeholder and date-only UTC corrections were
also inspected and tested. Required execution metadata rejects whitespace-wrapped
unknown sentinels and requires an actual timezone-aware UTC date/time.

## Independent validation

```text
python3 -m unittest discover -s tests -p 'test_research_harness.py' -v
32 tests passed in 0.125 seconds

python3 tools/validation/fixtures.py check
PASS: original synthetic fixtures match generator

git diff --check
PASS
```

The reviewer checked Friis values, additive wall/slab losses, linear-power
coupling, uplink/downlink asymmetry, TIN/hull behavior, toy roam hysteresis,
finite set-cover optimality and permutations, and synthetic provenance. Runtime
evidence checks reject missing requirements, wrong evidence kinds, wrong pinned
versions, bad hashes, escapes/symlinks, oversized artifacts, and invalid status
summaries. A valid `NOT_RUN` result returns exit 2, never a passing gate exit 0.
Independent adversarial probe files remain in temporary directories; no target
source was changed by the reviewer and no recursive deletion was performed.

## Approval limits

The catalog contains 20 gates and 265 individual checks, all initially `NOT_RUN`.
Its procedures do not implement or execute collector/worker acceptance drivers.
The 24 canonical scenes, 25-point artificial survey, and single-triangle TIN
oracle are test infrastructure. They do not prove any production engine or UI
implements those workflows, and they are not captured or calibrated RF data.

Reflection/diffraction electromagnetic coefficients and radio maps deliberately
remain unknown. Sionna/Kismet revisions are plan pins, not tested runtime version
claims. No upstream GPL implementation or fixture is reused. Fixture licensing
remains explicitly `NOASSERTION` pending the distribution decision.

Checksums and schema validation establish structure/integrity, not scientific
truth or authenticity. Review of actual hardware/field reports remains mandatory.
Product fixture consumption, real runtime drivers, cross-platform execution,
measured holdouts, calibration, and high-fidelity numerical validation remain
open requirements.
