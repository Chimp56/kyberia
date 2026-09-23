# Map asset admission validation

This packet covers a non-desktop initial raster and mutation increment for
plan §5.3, MAP-002, MAP-003, MAPB-001 and MAPB-002. It does not claim the
complete map import catalog, desktop workflow, or Phase 0 exit.

## Evidence in the bounded increment

- Strict PNG container admission checks the signature, checked chunk lengths,
  CRCs, IHDR encoding, nonzero bounded dimensions, pixel count, palette and
  transparency relationships, metadata bytes, chunk work, IEND and exact EOF.
- Unsupported content, zero/extreme declarations, truncation, malformed
  lengths, CRC substitution, unknown/textual metadata and trailing polyglot
  bytes fail closed. A PNG remains a PNG when untrusted `.jpg`/`image/jpeg`
  hints are supplied.
- The admitted ancillary subset follows the ordering constraints in
  [PNG 3 §5.6, Table 7](https://www.w3.org/TR/png-3/): `gAMA` and `sRGB`
  precede `PLTE`/`IDAT`, `tRNS` follows `PLTE` when that optional chunk occurs,
  and `pHYs` precedes `IDAT`. Valid-length, CRC-correct fixtures cover both
  accepted order and rejection of `sRGB`/`gAMA` after `PLTE`, truecolor `tRNS`
  before a later `PLTE`, and `pHYs` after `IDAT`.
- Admission never inflates IDAT. Hash, byte length, dimensions and
  `image/png` type are derived from content; source paths are not returned.
  Therefore a passing container admission does not establish a valid zlib/
  DEFLATE stream or decodable pixels. Any future renderer must decode
  successfully before treating the source as displayable.
- V3 map import and calibration operations round-trip through application,
  durable storage, materialization, publication and reopen. Exact retries are
  idempotent; stale revisions, cancellation and read-only mode do not publish
  a new current project.
- Two-point transform output is checked against a coordinate oracle. Binding
  floor evidence makes a later scale change fail preflight without publishing
  the rejected operation. Concurrent evidence binding and V3 calibration
  conflict in both operation-ID orders, including when the map was imported
  earlier in the same operation set. Another-floor calibration remains
  independent, and V3 undo restores the explicit `Unknown(NotMeasured)` prior.
- Existing V1 golden identity and V2 typed-prior tests remain in the focused
  package suite. Durable source closure is checked for missing, mismatched,
  corrupt, wrong-media-type and wrong-length artifacts at relevant storage
  boundaries.

## Executed checks

- `cargo test --locked --offline -p kyberia-application map_asset::tests -- --test-threads=1` — PASS (11/11 PNG admission tests, including five normative-order fixtures).
- `cargo test --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application -- --test-threads=1` — PASS (334 unit/integration tests and 8 doctests; two explicit benchmark tests ignored).
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application --all-targets -- -D warnings` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 locked external packages.
- `python3 tools/ledger.py generate` and `python3 tools/ledger.py check` — PASS; generated traceability is current (5,396 plan source blocks, 438 explicit ID occurrences and 447 headings).
- `git diff --check` — PASS.

The full workspace Rust suite was not run for this bounded increment. The
listed focused checks were independently rerun for the PNG-ordering correction;
the broader candidate review and correction re-review are recorded in the
review packets below. No product acceptance or Phase 0 exit is implied.

## Mainline promotion verification

The reviewed code was integrated at `2670207`; validation was rerun on the
mainline tree after the Wi-Fi increment was already present. The five-crate
suite passed 334 unit/integration tests and 8 doctests (2 explicit benchmark
tests ignored). Strict Clippy, workspace formatting, architecture, the locked
source inventory (522 packages), ledger (5,396 source blocks) and
`git diff --check` also passed. The full workspace suite was not run.

The author handoff records exact command outputs and limitations. Focused
validation uses the domain, operation-log, causal-materializer, project-store
and application suites, followed by formatting, strict Clippy, architecture,
locked source inventory and ledger checks. The broader review initially
requested changes for PNG ancillary ordering; the correction re-review found
that issue resolved with no new finding in scope. See
[`map-asset-admission-current-review.md`](../reviews/map-asset-admission-current-review.md)
and [`map-asset-order-fix-rereview-20260923.md`](../reviews/map-asset-order-fix-rereview-20260923.md).

## Limitations

Compressed pixel-stream decodability is not established at admission because
admission deliberately does not decompress attacker-controlled pixels. Other
raster/vector/PDF/CAD formats, previews, normalization, desktop UI,
multi-point calibration, residual reporting, CRS selection and evidence
migration remain open and must not be inferred from this packet.
