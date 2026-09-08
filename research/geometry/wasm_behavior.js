// Execute the compiled wasm32-unknown-unknown cdylib with Node's WebAssembly runtime.
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

const wasmPath = process.argv[2];
const outputPath = process.argv[3];
if (!wasmPath || !outputPath) {
  throw new Error("usage: node wasm_behavior.js <wasm> <output.json>");
}

const repoRoot = process.cwd();
const sourcePaths = [
  "research/geometry/fixtures/geometry-proof.json",
  "research/geometry/fixtures/geometry-proof-input.geojson",
  "research/geometry/Cargo.toml",
  "research/geometry/Cargo.lock",
  "research/geometry/src/main.rs",
  "research/geometry/src/lib.rs",
  "research/geometry/src/import.rs",
  "research/geometry/shapely_proof.py",
  "research/geometry/benchmark.py",
  "research/geometry/wasm_behavior.js",
  "research/geometry/shapely-requirements.txt",
];
const sha256 = (bytes) => crypto.createHash("sha256").update(bytes).digest("hex");
const sourceHashes = Object.fromEntries(sourcePaths.map((source) => [source, sha256(fs.readFileSync(path.join(repoRoot, source)))]));
const fixtureSha256 = sourceHashes["research/geometry/fixtures/geometry-proof.json"];
const geojsonFixtureSha256 = sourceHashes["research/geometry/fixtures/geometry-proof-input.geojson"];
const wasmBytes = fs.readFileSync(wasmPath);

WebAssembly.instantiate(wasmBytes, {}).then(({ instance }) => {
  const exports = instance.exports;
  const result = {
    runtime: "node-webassembly",
    wasm_path: wasmPath,
    wasm_sha256: sha256(wasmBytes),
    source_revision: process.env.KYBERIA_GEOMETRY_SOURCE_REVISION || "UNCOMMITTED_WORKTREE_CONTENT_HASH_BOUND",
    source_hashes: sourceHashes,
    fixture_sha256: fixtureSha256,
    geojson_fixture_sha256: geojsonFixtureSha256,
    semantics_version: exports.geometry_semantics_version(),
    operations: {
      union_area: exports.geometry_union_area(),
      intersection_area: exports.geometry_intersection_area(),
      difference_area: exports.geometry_difference_area(),
      buffer_area: exports.geometry_buffer_area(),
      hole_span_length: exports.geometry_hole_span_length(),
      crossing_x: exports.geometry_crossing_x(),
    },
  };
  fs.writeFileSync(outputPath, JSON.stringify(result, null, 2) + "\n");
}).catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
