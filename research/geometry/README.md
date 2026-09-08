# Geometry proof harness

This directory is a bounded research harness for Gate E. It compares the
portable Rust `geo`/`geojson` stack with GEOS through Shapely on an original
GeoJSON/WKT fixture. It is deliberately outside the Cargo workspace and has no
production dependency role.

Run the proof and inspect the retained results with:

```sh
cargo build --manifest-path research/geometry/Cargo.toml --locked --offline --release
cargo build --manifest-path research/geometry/Cargo.toml \
  --target wasm32-unknown-unknown --locked --release --offline
cargo test --manifest-path research/geometry/Cargo.toml --locked --offline
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
KYBERIA_GEOMETRY_RUNTIME_GATE=1 python3 -m unittest tests.test_geometry_research
```

The Python proof environment is populated from the hash-pinned
`shapely-requirements.txt`. Build outputs stay under ignored `target/` and
`.tools/` directories. The fixture and result contracts are explained in
[`docs/validation/geometry-proof.md`](../../docs/validation/geometry-proof.md).
The repository-wide Python suite skips only the two optional runtime checks
when those ignored artifacts are absent. The explicit Gate E command above
turns absence into a failure after the documented build and environment setup.
