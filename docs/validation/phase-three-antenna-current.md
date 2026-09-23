# Bounded Phase 3 antenna-pattern validation

The importer/evaluator is integrated on `main` at `6709813`; the non-pole
elevation-interpolation golden is at `c016288`. Independent review and
follow-up are recorded at `0d065d4` and `0ac5efb`. The reviewed candidate was
based on `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`; exact source bytes are
recorded by content hash in the implementation ledger. The executable schema
validator is integrated at `22b8b33`, with trailing-LF corrections at
`11bd549` and `7986171`; reviews are recorded at `e6fbde4`, `a094b83`, and
`1881ba4`.

This validation covers only the versioned JSON importer and evaluator in
`crates/antenna-model`. The tests use synthetic patterns; there is no licensed
manufacturer dataset, visual normalization review, full product integration,
or Sionna execution in this evidence.

| Command | Result and scope |
| --- | --- |
| `cargo test -p kyberia-antenna-model --locked --offline` | PASS: 11 integration tests; includes a local +X/45° elevation golden and trailing-line-feed rejection in all four schema-patterned text fields. No unit or doctests are defined. |
| `cargo test -p kyberia-antenna-model --locked --offline direction_elevation_and_frequency_interpolation_are_in_linear_power -- --exact` | PASS: targeted non-pole elevation golden. |
| `cargo clippy -p kyberia-antenna-model --all-targets --locked --offline -- -D warnings` | PASS: all crate targets, warnings denied. |
| `cargo fmt --all -- --check` | PASS: candidate workspace formatting. |
| `python3 tools/architecture.py check` | PASS: reviewed dependency directions and external package boundaries. |
| `python3 tools/source_inventory.py check` | PASS: 522 locked external packages. Count was 522 before and after this local crate addition; the generated inventory diff updates only the Cargo.lock SHA-256. |
| `python3 tools/ledger.py check` | PASS: 5,396 source blocks, 438 explicit ID occurrences, 447 headings. |
| `python3 -m unittest discover -s tests -p 'test_ledger.py'` | PASS: 31 ledger tests. |
| `git diff --check` | PASS: candidate working-tree whitespace check. |
| `python3 tools/dev.py validate-antenna-schema` | PASS on integrated `main` after corrections `11bd549` and `7986171`, using the existing pinned `jsonschema==4.25.1` interpreter: `Draft202012Validator.check_schema` accepts the committed schema; the Rust-shared JSON fixture and all-absent optional cross-polar plane are accepted; eight invalid instances are rejected, including trailing line feeds in all four schema-patterned text fields. |

The integrated-tree run used the already-installed interpreter from the
checkout:

```sh
KYBERIA_TOOL_PYTHON=/Users/vincent/code/kyberia/.tools/supply-chain-schema-venv/bin/python \
  python3 tools/dev.py validate-antenna-schema
```

The candidate worktree did not contain a generated Python environment. The
run above does not establish a clean bootstrap. In a fresh checkout, set up the
standard environment and then install the pinned supply-chain extras:

```sh
python3 tools/dev.py bootstrap
python3 tools/dev.py supply-chain-bootstrap
python3 tools/dev.py validate-antenna-schema
```

Normal `bootstrap` alone does not install the optional, hash-pinned
supply-chain validator. The explicit command checks for exactly
`jsonschema==4.25.1` and fails with a nonzero result when it is absent or at
another version.

The fail-closed path was also exercised with a configured Python that has no
`jsonschema` package: the command returned exit code 2 with the bootstrap
instruction above and did not attempt a package install.

The schema's complete set of four regular-expression patterns was audited
against the corresponding Rust validation path:

| Schema field | Rust validation path | Schema/runtime relationship |
| --- | --- | --- |
| `model_id` | `validate_identifier` → `validate_text` | Printable ASCII, bounded length, and no leading/trailing whitespace; interior ASCII spaces are allowed. |
| `source.source_uri` | `validate_source_uri` → `validate_text` | Supported scheme and nonempty printable-ASCII suffix agree. |
| `source.license_spdx` | `validate_text`, then `valid_spdx_expression` | Schema checks bounded printable ASCII; SPDX grammar remains a stricter runtime check. |
| `source.source_checksum_sha256` | `validate_sha256` | Exactly 64 lowercase hexadecimal ASCII bytes. |

Each regex now requires true end-of-input after its ASCII content; `$` alone
can match before a final line feed in the pinned validator. JSON Schema also
does not encode cross-field/runtime invariants, which remain checked by Rust.

The evidence exercises explicit axes/rotation, pole and flattening behavior,
the non-pole elevation golden and spatial/frequency interpolation,
co/cross-plane availability, uncertainty and
provenance fields, canonical signed-zero/quaternion identity, malformed inputs,
and parser/work bounds. It does not establish vendor-format compatibility,
license entitlement or source-checksum correspondence, polarization mismatch
loss, cuts/harmonics, visualization, field calibration, or adapter behavior.

The independent review approved the earlier bounded synthetic
contract/evaluator increment with no BLOCKER or MAJOR finding. Its
elevation-interpolation MINOR follow-up is resolved by the golden test. The
integrated schema validator executes the structural Draft 2020-12 conformance
check. Relevant ledger records remain `IN_PROGRESS`; this evidence does
not establish clean-bootstrap execution, semantic Rust validation through
JSON Schema, vendor-format compatibility, or complete Phase 3 acceptance.

The existing Rust schema test still checks JSON parsing and selected structural
fields. The separate developer command is the standards-validator check; the
Rust importer continues to enforce cross-field semantics the schema cannot
express, such as strict ordering, matching grid dimensions, unit-quaternion
norm, and gain normalization. The original independent review records the
validator gap as it existed at that review's revision.
