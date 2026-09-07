# Implementation ledger contract

Run from the repository root using Python 3.9 or newer:

```sh
python3 tools/ledger.py generate
python3 tools/ledger.py check
python3 -m unittest discover -s tests -p 'test_ledger.py'
```

`ledger.json` is the editable status/evidence record. `TRACEABILITY.md` is generated. `execution-dag.json` is an editable, checked work graph. Generation preserves existing status/evidence when the source hash is unchanged and refuses automatic reconciliation when the plan changes. The checker never edits files.

## Lossless source coverage

The initial plan has 6,201 lines, 446 headings, and 438 explicit requirement/ADR definition occurrences. They become 5,392 blocks: headings, prose paragraphs, individual list items, individual table rows, fenced examples/diagrams, separators, and whitespace. Concatenating their exact `source.text` fields reconstructs `plan.md`. The checker independently re-extracts the plan and compares every block, range, hash, heading ancestry, and inventory. Omissions, additions, edited text, changed ancestry, and stale hashes fail.

This intentionally includes contextual research, examples, and formatting. **Source block counts are not feature counts.** `structure` and `context` identify known scaffolding/bibliography and have `status: null`; these never need to be "implemented." Other prose is conservatively retained as reviewable leaf `obligation` records, while headings become `obligation_group` records. A quotation, diagram, or research question is not necessarily an independent production feature: its applicability still requires review. Traceability separately reports leaf obligations, groups, and coverage-only records. Parent/child links preserve the full hierarchy, and implemented/validated groups cannot hide unfinished mandatory descendants. Implementers attach acceptance cases to applicable obligations and explicitly cross-reference equivalent source occurrences.

There are 341 unique original IDs and 97 repeated original IDs. Namespaces preserve source meaning:

| Namespace | Source |
|---|---|
| `catalog:` | Capability catalog definitions |
| `backlog:` | Section 18 backlog definitions |
| `audit:` | Appendix I subsystem definitions |
| `adr-proposal:` | Section 20 ADR proposals |

An occurrence suffix prevents duplicate definitions within one namespace from colliding. For example, `backlog:FND-001:1` is domain units, while `audit:FND-001:1` is desktop lifecycle. `adr-proposal:ADR-012:1` and `adr-proposal:ADR-012:2` preserve the two different original ADR-012 proposals. Never join records by bare `original_id` alone.

Other block IDs combine kind, a content hash, and occurrence. Moving an unchanged unique block preserves its ID; editing its content changes its identity. Repeated identical content requires explicit reconciliation if occurrences change. Source line numbers are locators, not identity. Full SHA-256 hashes verify content; shortened hashes are only ID components.

The Appendix I matrices remain intact: 80 repository rows and 172 subsystem rows, with exact primary-disposition counts and row references. The source audit does not constitute runtime validation of upstream software.

## Evidence and statuses

All initial obligation/group records are `NOT_STARTED`; coverage-only records have no obligation status. Ledger tooling itself does not implement the product requirements it inventories.

| Status | Mechanically required evidence |
|---|---|
| `NOT_STARTED` | Source coverage only |
| `IN_PROGRESS` | Named owner |
| `IMPLEMENTED` | Owner, acceptance cases, implementation files, independent review |
| `VALIDATED` | Implemented requirements plus passing command/result/scope/revision evidence; required dependencies validated |
| `BLOCKED_EXTERNAL` | Specific dependency/reason, hashed evidence, implementation links, passing contract-test evidence, exact resumption procedure |
| `DEFERRED_BY_ADR` | Hashed accepted ADR file, approver, rationale, and explanation of preserved product intent |

All evidence records contain `path`, `description`, full immutable `revision` (40-digit Git hash or 64-digit content hash), and file `sha256`. The current file content must match that digest. Reviews additionally identify `reviewer`, different from the author/owner, `disposition: APPROVED`, and an explicit `findings` list. BLOCKER findings must be `RESOLVED`; MAJOR findings must be `RESOLVED` or `ACCEPTED_BY_ADR` with accepted ADR evidence. A review request for changes never qualifies as approval. Validation entries also contain `command`, `result` (`PASS` for validated requirements), and `scope`.

An ADR evidence record includes `decision_status: ACCEPTED`, `accepted_by`, `reason`, and `preserved_product_intent`; the file must be under `docs/architecture/ADR/`. Blocker records include `dependency`, `reason`, `resume_procedure`, an `evidence` record, and nonempty `implementation` and `contract_validation` evidence lists. Contract results require `scope: contract`, a named command, and `result: PASS`. Unsupported hardware is not evidence of a completed interface. Traceability renders blocker and deferral explanations next to their links.

Paths are repository-relative existing files; absolute paths, traversal, and symlink escapes are rejected. The plan and implementation ledger do not qualify as implementation evidence. Optional URL fragments are locators; the checker verifies the file, not Markdown anchor resolution.

For example, an implementation entry can point to a crate file, a validation entry to a checked-in test report recording the exact revision/environment/command, and a review entry to an independent findings report. Reports must identify unresolved findings and their resolution. An author cannot be their own sole reviewer.

The checker validates evidence structure, current content digests, and explicit review/ADR decisions, **not the truth or sufficiency of prose assertions**. It cannot establish that a test report is authentic, that an implementation is substantive, that an asserted ADR approval was authorized, or that an author/reviewer identity represents two independent people. Independent code review and the §23 definition of done remain mandatory. Passing this checker is not final product acceptance.

Use separate acceptance cases and evidence scopes for contract, replay, native runtime, hardware, numerical, measured holdout, UI, migration/recovery, security, and performance validation. A passing mock cannot satisfy a live-radio or calibrated measurement gate. Keep a broad group `IN_PROGRESS` while a required descendant remains externally blocked; completed independent leaf implementations retain their own status. Split source obligations into named acceptance cases when only one hardware-dependent clause is blocked. Do not classify an entire subsystem blocked solely because one hardware gate is unavailable.

`depends_on` and `related_requirements` reference occurrence-qualified IDs. The former encodes prerequisites and must be acyclic; the latter records overlap without implying equivalence or completion. Acceptance cases should include units, unknown states, provenance, capabilities, algorithms, uncertainty, export, UI, security, performance, migration and recovery where applicable, following §23 and Appendix E.

## Execution DAG

Nine delivery nodes preserve Phase 0 through Phase 8 order. Independent Phase 0 proof nodes cover contracts, storage, native capabilities, Kismet offline/live ingestion, spatial/renderer work, active diagnostics, Sionna, and neutral interchange. Kismet live work follows offline normalization. All proof nodes depend on canonical contracts rather than each other unnecessarily.

The graph orders deliverables; it does not declare a phase passed or forbid independent implementation while a prior hardware validation gate remains open. Phase 0 still requires real capability probes, persistence, export, and cross-platform deterministic evidence. Phase 1 still requires the complete usable workflow, including continuous paths and snapshots. Never advance the claimed completion phase merely because the next implementation work is runnable.

The checker rejects missing references, cycles, duplicate node IDs, missing ownership/acceptance, and omitted sequential phase dependencies. The initial graph is a prerequisite/delivery graph; each iteration should add narrower owned execution nodes as requirements are selected. It is not an assertion that every one of thousands of source blocks is already decomposed into an atomic engineering task.

Initial ownership/phase defaults follow explicit phase headings first, then requirement families and domain sections. They are scheduling classifications, not claims that all items in a family ship at the same time. Narrow them when selecting an iteration. General research/contextual obligations use `specification/review` with no delivery phase pending applicability review. Coverage-only records have no owner or phase. Original source identity and text remain immutable while owner/phase can be refined.

## Updating a changed plan

Keep source changes reviewable. Independently extract the new plan to a temporary artifact using `extract()`, compare changed/removed source blocks and their previous evidence, and explicitly reconcile statuses and links. Retain archival evidence for superseded obligations. Replace the ledger only after review, then regenerate traceability. Automatic regeneration deliberately refuses to erase statuses or silently apply old evidence to changed requirements.

Initial baseline and specification findings are recorded in [initial-audit.md](../validation/initial-audit.md) and [initial-spec-audit.md](../reviews/initial-spec-audit.md).
