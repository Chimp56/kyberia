# Gate E geometry-kernel proof

This is the Phase 0 Gate E research record for `plan.md` §20. The result is a
provisional portable 2-D boundary proposal pending independent review. It
compares Rust `geo`/`geojson`/`wkt` with GEOS through Shapely on an independently
authored fixture. The harness is under `research/geometry/`; it is outside the
Cargo workspace and adds no production dependency.

The fixture is floor-local planar metres. It contains a polygon with a hole,
overlapping polygons, ground and upper walls with identical XY, crossing,
touching, and collinear lines, and a self-intersecting bow-tie. Floor and
`z_m` properties are application metadata. Neither backend is used as a 3-D,
CRS, material, CAD/BIM, or building kernel.

## Import and operation boundary

The actual untrusted import entry point is the Rust
`geometry-proof import <input.geojson> <output.json>` command. The raw input is
`research/geometry/fixtures/geometry-proof-input.geojson`; the surrounding
fixture is retained separately for expected values. The importer checks the
256 KiB byte bound before parsing, scans whole-document JSON nesting, rejects CRS members,
requires finite 2-D coordinates, and exhaustively counts positions for Point,
MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon, and
GeometryCollection. It bounds 10,000 positions, 1,024 features, 4,096
geometries, and 64 nested geometry levels. It rejects malformed or unbalanced
JSON, unsupported types, Z/M coordinates, non-finite numbers, null feature
geometries, empty collections, short lines, and unclosed rings. The Python
comparison path applies the same checks before converting to Shapely.
Bounding boxes are validated identically as finite four-value 2-D boxes. Python
also catches integer-to-float `OverflowError` while checking enormous numeric
coordinates and reports the same clean rejection class as other malformed
inputs. The focused suite exercises malformed bbox shapes and an integer larger
than any representable float.

The file reader does not trust a pre-read stat size. It fills a buffer capped at
256 KiB and checks only a one-byte size sentinel before parsing; an oversized
file is rejected without an allocation proportional to its advertised size,
and the sentinel is discarded. Valid-file hashes are computed from the exact
bytes retained by the parser. A 70-level nested `properties` document is also
rejected before geometry conversion, proving the Python whole-document depth
bound rather than only a GeometryCollection depth bound.

The operation harness validates ground, WKT A, WKT B, and the buffer square
before any boolean or offset. The bow-tie is diagnosed first. Rust retains the
diagnostic and explicitly does not repair. Shapely's repair is an explicitly
invoked `make_valid`; the retained record includes the source WKT hash, repair
WKT, repair hash, result validity/type, method, Shapely version, and GEOS
version. There is no silent repair or `buffer(0)` repair.

Features are filtered by `floor` before conversion and operation. Exactly one
ground wall is selected. The upper wall has the same XY and is counted as
excluded before the 2-D intersection, proving that floor separation is an
application invariant rather than a library claim. The ground wall is a valid
three-vertex LineString; Rust iterates both wall segments and reports the
processed segment count, while Shapely receives the complete LineString. Rust
crossing, touch, collinear-overlap, and hole-span probes are all constructed
from named fixture values and retained under `operations.probe_inputs`; tests
compare those values back to the fixture so a fixture edit cannot silently
disconnect the probes.

## Locked commands

Commands run from the repository root were:

```sh
cargo check --manifest-path research/geometry/Cargo.toml --locked --offline
cargo test --manifest-path research/geometry/Cargo.toml --locked --offline
cargo clippy --manifest-path research/geometry/Cargo.toml --locked --offline \
  --all-targets -- -D warnings
cargo build --manifest-path research/geometry/Cargo.toml --locked --offline --release
cargo build --manifest-path research/geometry/Cargo.toml \
  --target wasm32-unknown-unknown --locked --offline --release
research/geometry/target/release/kyberia-geometry-proof import \
  research/geometry/fixtures/geometry-proof-input.geojson \
  research/geometry/results/import-valid.json
research/geometry/target/release/kyberia-geometry-proof \
  research/geometry/results/rust-desktop.json
.tools/geometry-venv/bin/python research/geometry/shapely_proof.py \
  research/geometry/results/shapely.json
node research/geometry/wasm_behavior.js \
  research/geometry/target/wasm32-unknown-unknown/release/kyberia_geometry_proof.wasm \
  research/geometry/results/wasm-behavior.json
.tools/geometry-venv/bin/python research/geometry/benchmark.py
python3 -m unittest tests.test_geometry_research
```

The commands passed on macOS 26.6.2 arm64 with Rust 1.98.1, target component
`rust-std 1.98.1 wasm32-unknown-unknown`, Node's WebAssembly runtime, Python
3.9.6, Shapely 2.0.7, and GEOS 3.11.4. The compiled WASM artifact is ignored;
`wasm-build.json` retains its path, size, SHA-256, locked build command,
behavior command, and behavior result hash. The behavior result itself retains
the runtime name, WASM hash, exact source map, source revision, fixture hashes,
and semantic outputs. The focused acceptance test requires the rebuilt WASM
file to be present during the normal gate and verifies its SHA-256.

## Retained semantic results

| Operation | Independent expected result | Rust `geo` | GEOS/Shapely |
|---|---:|---:|---:|
| Shell minus 4×4 hole | 84.0 m² | 84.0 | 84.0 |
| Polygon intersection | 25.0 m² | 25.0 | 25.0 |
| Polygon union | 175.0 m² | 175.0 | 175.0 |
| Polygon difference | 75.0 m² | 75.0 | 75.0 |
| 4×4 square buffered by 1 m, 8 segments/quadrant | 35.1416 m² analytic; 0.03 m² tolerance | 35.121445 | 35.121445 |
| Hole-crossing line inside length | 6.0 m | 6.0 | 6.0 |
| Crossing intersection | (5, 5) | point | point |
| Endpoint touch | (5, 0) | point | point |
| Collinear overlap | (2, 0)–(8, 0) | collinear segment | LineString |

The collinear type label differs while its canonical segment agrees. The
adapter must normalize topological variants rather than compare backend type
names. The desktop and WASM retained values agree for union, intersection,
difference, buffer, hole clipping, and crossing X; the focused Python tests
compare these values directly.

## Benchmark scope

The retained 20-iteration benchmark runs the same bounded GeoJSON import,
WKT validation, floor-filtered selection, polygon booleans, rounded buffer,
line cases, hole clipping, and invalid-geometry diagnostics in both processes.
It reports internal operation timing separately from process startup timing.
The internal timer endpoints are aligned, but Rust consumes compile-time
embedded bytes while Shapely reads fixture files, so internal medians are
descriptive per backend and are not a direct performance ranking. Current
internal medians are 0.3838 ms for Rust and 1.5906 ms for Shapely; process
medians are 3.7175 ms and 170.9192 ms. These are observations for one
developer machine and this fixture, not release thresholds or a claim about
unmeasured workloads. Every benchmark record includes the Rust binary
SHA-256, fixture SHA-256, exact source hash map, source revision marker, and
the hash of the exact pinned `shapely-requirements.txt` content recorded in the
source ledger.

## Provisional decision

The accepted proposal is to use `geo` plus `geojson` at a canonical portable
2-D Rust/application boundary with the bounded importer, validation-before-
operation rule, floor filtering above the kernel, normalized results, and
explicit repair provenance. Keep GEOS/Shapely as an optional research or
explicit repair adapter; the tested GEOS path is Python/native and its
LGPL-2.1 scope is not part of the desktop/WASM boundary.

This remains provisional until independent review accepts the evidence. It
does not select a 3-D kernel, CRS engine, CAD/BIM importer, material model, or
custom production geometry code. Floor/frame/material policy remains owned by
the application adapter. The fixture carries `material: concrete` as metadata;
the proof reports topological XY intersections only and makes no material
attenuation or material-intersection claim. A future canonical adapter must
join and retain material identity before any propagation operation.

## Sources and license scope

Direct pins for `geo`, `geojson`, `wkt`, `serde`, `serde_json`, and `sha2`, plus
the Shapely/GEOS/Numpy wheel/runtime hashes, redistribution statements, exact
Cargo lock hash, and transitive review limitation are in
[`geometry-research-sources.json`](../licenses/geometry-research-sources.json).
The exact transitive package graph is frozen by `Cargo.lock`; direct licenses
were recorded, while package-by-package transitive license text review was not
completed. The ledger explicitly says this is not an SBOM and is not a
redistribution approval.

Primary references are [`geo` 0.33.1](https://docs.rs/crate/geo/0.33.1),
[`geojson` 1.0.0](https://docs.rs/crate/geojson/1.0.0),
[Shapely 2.0.7](https://shapely.readthedocs.io/en/2.0.7/), and
[GEOS licensing](https://libgeos.org/usage/download/). The available isolated
interpreter is Python 3.9.6, so the proof pins the compatible Shapely 2.0.7
arm64 wheel; changing Python, GEOS, Shapely, or the Rust pins requires a new
source map and the complete validation set.
