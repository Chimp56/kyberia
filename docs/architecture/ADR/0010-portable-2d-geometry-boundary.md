# ADR 0010: Provisional portable 2-D geometry boundary

Status: Accepted proposal; Gate E evidence remains provisional pending independent review

## Context

Kyberia needs a deterministic geometry boundary for floor-local planar work:
bounded GeoJSON/WKT import, polygon booleans and offsets, wall and line
intersections, holes, validity diagnostics, and separation of floor metadata
before 2-D operations. The boundary must build for desktop and
`wasm32-unknown-unknown`. The 2-D libraries do not model floors, materials,
CRS transforms, or 3-D/Z coordinates, so those concerns remain application
contracts around the kernel.

The bounded file reader caps retained input at 256 KiB and uses a one-byte
sentinel to reject oversized files without trusting a pre-read stat or
allocating from an advertised file size. Hashes are taken from the exact bytes
retained and parsed. Python applies the same whole-document nesting bound,
including nested properties.

The Phase 0 research harness uses an independently authored fixture and keeps
all result and benchmark records tied to exact fixture, source, and lockfile
hashes. Its importer accepts the seven GeoJSON geometry variants only after
byte, nesting, feature, geometry, coordinate, cardinality, finite-number, CRS,
and dimensionality checks. It filters the ground floor before geometry work;
the upper wall deliberately has identical XY and is retained only as excluded
floor metadata. Invalid geometry is diagnosed before operations. Rust `geo`
rejects repair; the optional Shapely/GEOS path performs an explicit
`make_valid` and retains the source hash, repair artifact hash, method, and
versions.

## Decision

Accept `geo` 0.33.1 with `geojson` 1.0.0 and `wkt` 0.14.0 as the proposed
portable 2-D Rust boundary for the canonical application adapter. The adapter
must:

1. enforce bounded, finite, strictly 2-D import and reject CRS, Z/M, malformed,
   unsupported, oversized, null, empty, unclosed, and over-nested inputs;
2. select the requested floor and coordinate frame before invoking a 2-D
   operation, carrying floor and material provenance outside the library;
3. validate every geometry before operations and fail closed on invalid input;
4. preserve normalized operation results and exact source/revision hashes;
5. expose explicit repair as a separate operation that produces a
   content-addressed artifact and provenance record; and
6. keep GEOS/Shapely optional and out of the desktop/WASM production boundary.

The proposal covers planar XY only. It does not select a 3-D kernel, CRS
engine, CAD/BIM importer, material intersection policy, ray tracer, or custom
production geometry implementation. No library result may be interpreted as
separating same-XY geometry on different floors.

The fixture carries a material identity only as application metadata. The
proof reports topological XY intersections and does not claim material
attenuation or material intersection behavior; a future propagation adapter
must join and retain material identity before using a geometry result.

## Alternatives considered

- **GEOS/Shapely as the canonical kernel:** stronger validity diagnostics and
  `make_valid`, but the exercised path is Python/native and introduces the
  GEOS LGPL-2.1 licensing and WASM/runtime boundary. Keep it as an explicit
  research or repair adapter.
- **Rust `geos` bindings:** could expose GEOS directly, but adds native
  linkage and the same LGPL/runtime constraints to the portable boundary.
- **A custom production geometry kernel:** rejected for Phase 0 because it
  expands numerical, topology, repair, and maintenance risk without evidence
  that the adopted libraries are insufficient.
- **A 3-D/CAD/BIM library:** outside this Gate E question; floor and material
  semantics remain application-owned contracts.

## Evidence

- [Gate E validation record](../../validation/geometry-proof.md)
- [retained Rust result](../../../research/geometry/results/rust-desktop.json)
- [retained Shapely/GEOS result](../../../research/geometry/results/shapely.json)
- [retained bounded import result](../../../research/geometry/results/import-valid.json)
- [retained WASM behavior result](../../../research/geometry/results/wasm-behavior.json)
- [retained benchmark](../../../research/geometry/results/benchmark.json)
- [source and license ledger](../../licenses/geometry-research-sources.json)

The proof executes the compiled release WASM artifact in Node's WebAssembly
runtime and compares union, intersection, difference, buffer, hole clipping,
and crossing semantics to the desktop result. The build artifact remains
ignored; its path, byte count, SHA-256, runtime behavior result, and exact
source map are retained. The normal acceptance gate requires that rebuilt
artifact to be present before accepting the behavioral evidence.

## Consequences

The proposal gives the application a small portable 2-D boundary with bounded
untrusted import, deterministic fixture evidence, and a clear place for floor,
frame, material, and repair policy. It avoids a native GEOS dependency in the
desktop/WASM path. It also leaves repair capability optional, requires
provenance for any repair artifact, and does not solve 3-D, CRS, CAD/BIM, or
material semantics. Benchmark values are observations for the retained
workload and machine, not performance thresholds.

The direct dependency licenses and exact Cargo transitive graph are recorded;
the transitive license review is explicitly incomplete and is not an SBOM or a
redistribution approval.

## Reversibility

This is an adapter and research dependency decision. It can be reversed by
replacing the adapter behind its normalized contracts and rerunning the same
fixture, importer, invalidity, WASM, license, and benchmark evidence. The
fixture and result schemas should remain stable during a replacement so
semantic differences are visible.

## Validation

Independent review must verify the owned files, rerun the locked offline Rust
checks and behavioral Node WASM command, rerun the hash-bound Shapely and
benchmark commands, inspect the negative importer cases, and confirm that the
Gate language remains provisional until that review accepts the evidence. A
future dependency, interpreter, GEOS, toolchain, or repair-method change
requires a new source map and this validation set before changing the status.
