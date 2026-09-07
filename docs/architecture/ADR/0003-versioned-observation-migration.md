# ADR 0003 — Explicit observation and point-receipt migration

Status: accepted after independent root review of implementation, tests and
adversarial decoder probes.

## Context

Plan §§3, 7.1, 10.9/10.12/10.16, 11.4–11.6 and Appendix I require canonical
ownership, explicit unknown provenance and versioned validating adapters. A
producing program's version may be unavailable in an imported database even
when the data schema is known. Observation V1 incorrectly required a nonempty
`Text` for `source.source_version`. Inventing a software version, copying a data
schema version into that field or rejecting otherwise valid evidence would lose
the specified semantics. The current point receipt copied that mandatory text.

The shared `SchemaVersion` enum is also used for projects, capabilities and
calibration. Expanding that enum globally would make unrelated decoders accept a
version whose semantics they have not implemented.

## Decision

Canonical observations use their own V2 schema type. The software source version
is `Evidence<Text>`. The data-format field `source_schema_version` remains
independent. No foreign model or runtime dependency crosses inward.

The affected wire fragments are:

```json
{"schema_version":"1","source":{"source_version":"2025.09"}}
```

```json
{"schema_version":"2","source":{"source_version":{"state":"known","detail":"2025.09"}}}
```

```json
{"schema_version":"2","source":{"source_version":{"state":"unknown","detail":"source_did_not_provide"}}}
```

These are fragments; all other required envelope fields remain mandatory. The
typed wire decoder recognizes exactly V1/V2, validates every field and applies
the same envelope invariants to both. Only the changed version scalar uses a
private wire union; its enclosing schema selects the admissible shape. V1 text
maps to known evidence. V2 evidence preserves its exact unknown reason. Missing,
null, mismatched, malformed or future versions fail; they do not select fallback.
Canonical construction and serialization emit only V2. Shared domain V1 schemas
remain closed and unchanged.

Point snapshots get a separate top-level `schema_version: "2"`, with receipt
source versions changed to evidence. Their nested capture configuration stays
V1. The exact original snapshot had no top-level version field; only that absent
tag with legacy textual records is accepted as historical V1. Explicit `"1"` was
never a published snapshot shape and is rejected. Tagged V2 rejects old record
values. Each migrated snapshot must pass current temporal, pose, uniqueness,
capability and completion invariants. Empty untagged snapshots have no changed
record shape to distinguish them and are explicitly classified as legacy V1.

`DecodedObservation` and `DecodedPointSurvey` return typed decoder receipts with
input schema, output schema, decoder version and migration flag. The application
persists those receipts alongside the immutable input-artifact reference when
storing a transformed artifact. The pure core neither hashes nor rewrites files.
IDs, timestamps and provenance values remain unchanged except for the explicit
source-version wrapper. New serialized artifacts have different byte hashes;
they must not replace immutable original evidence at its old content hash.
Decoder receipts are provenance, not authentication or historical signatures.

## Alternatives

- Keep mandatory text and require sidecar attestations: unnecessarily prevents
  importing evidence whose software version cannot be recovered.
- Use a sentinel string: turns an unknown reason into a fabricated version.
- Accept text or evidence within unchanged V1: conceals a semantic wire change
  and leaves persisted point snapshots without an explicit migration policy.
- Add V2 to shared `SchemaVersion`: accidentally broadens unrelated contracts.
- Rewrite persisted input files automatically: violates immutable provenance and
  checksum identity; recovery and permissions also belong to outer storage.
- Drop V1 support because the project is pre-release: discards already reviewed
  fixtures and early project evidence without technical necessity.

## Evidence

Before changing any serializer, the reviewed V1 code at survey commit `6f41f74`
exported independently authored synthetic golden fixtures. Their exact UTF-8
bytes, including formatting and trailing newline, are retained:

| Fixture | SHA-256 |
|---|---|
| `crates/domain/tests/fixtures/observation-v1.json` | `d8eac4c4b797644bf257fcf9d5df3babe93ea4e7b7ceef1815691ab5c2b9c8ec` |
| `crates/survey/tests/fixtures/point-v1.json` | `787b8ae60d0d7cd79a383930648229d39f26de18b52658f01ee0e08fae1bb4e8` |
| `crates/survey/tests/fixtures/point-observation-v1.json` | `9155d9a7bb2dca5317c26807a57f949dc049ff98641053754edeff11bbfe759b` |

These are original synthetic fixtures generated from Kyberia's existing tests;
they contain no captured third-party evidence or external implementation data.
License status is `NOASSERTION`, consistent with the central fixture ledger while
the project distribution decision remains pending. A migration acceptance test
first failed against V1 serialization before
implementation. No production dependency was added.

## Consequences and reversibility

This intentionally changes the pre-release Rust construction API: adapters use
the observation-specific V2 tag and typed source-version evidence. Previously
serialized V1 observations and point snapshots remain readable. It does not
change capture timestamp semantics, upstream data schemas, numerical values,
project schemas, capability documents or application operation logs.

Old immutable artifacts and reviewed commits remain the evidence for V1 behavior;
new validation evidence must cite V2 tests and reviewed code. There is no V1
downgrade writer because V1 cannot represent unknown software versions honestly.
An explicit future export could reject unknown versions and separately define
loss policy, but it is not part of this change.

Outer import framing still must limit bytes/depth before Serde allocation and
validate artifact checksums and receipt-to-observation references. Migration
receipts do not supply those storage checks. The wire module and public receipt
types isolate future decoder changes. Rollback to an older binary can read only
retained V1 artifacts; it must reject newly written V2 rather than reinterpret it.

## Validation plan

Run formatting, Clippy, all domain/survey tests and independent review. Require:
golden V1-to-V2 equality except the specified fields; known/unknown preservation;
native-V2 versus migrated-V1 receipts; no unrelated V2 schema acceptance; missing,
null, mixed-shape, malformed and duplicate fields rejected; all old envelope and
point invariants enforced; arbitrary-byte decoder property tests; legacy snapshot
progress/reference preservation; and actual admission/save/read of an observation
whose source version is unknown. Verify the listed original fixture hashes after
implementation. Storage-level manifest/immutable-artifact migration integration
remains an outer application requirement.
