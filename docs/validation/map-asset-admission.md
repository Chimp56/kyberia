# Map asset admission validation

This packet covers a non-desktop initial raster and mutation increment for
plan §5.3, MAP-002, MAP-003, MAPB-001 and MAPB-002. It does not claim the
complete map import catalog, desktop workflow, or Phase 0 exit.

## Evidence in this candidate

- Strict PNG container admission checks the signature, checked chunk lengths,
  CRCs, IHDR encoding, nonzero bounded dimensions, pixel count, palette and
  transparency relationships, metadata bytes, chunk work, IEND and exact EOF.
- Unsupported content, zero/extreme declarations, truncation, malformed
  lengths, CRC substitution, unknown/textual metadata and trailing polyglot
  bytes fail closed. A PNG remains a PNG when untrusted `.jpg`/`image/jpeg`
  hints are supplied.
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

- `cargo test --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application -- --test-threads=1` — PASS (focused affected suites; one explicit throughput benchmark ignored).
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --locked --offline -p kyberia-domain -p kyberia-operation-log -p kyberia-causal-materializer -p kyberia-project-store -p kyberia-application --all-targets -- -D warnings` — PASS.
- `python3 tools/architecture.py` — PASS.
- `python3 tools/source_inventory.py check` — PASS, 522 locked external packages.
- `python3 tools/ledger.py check` — PASS, 5,396 source blocks, 438 explicit ID occurrences and 447 headings.

The full workspace Rust suite was not run for this bounded candidate. The
commands above are candidate-author evidence only, not independent review.

The author handoff records exact command outputs and limitations. Focused
validation uses the domain, operation-log, causal-materializer, project-store
and application suites, followed by formatting, strict Clippy, architecture,
locked source inventory and ledger checks. Independent review is pending.

## Limitations

Compressed pixel-stream decodability is not established at admission because
admission deliberately does not decompress attacker-controlled pixels. Other
raster/vector/PDF/CAD formats, previews, normalization, desktop UI,
multi-point calibration, residual reporting, CRS selection and evidence
migration remain open and must not be inferred from this packet.
