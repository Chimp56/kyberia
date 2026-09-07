# Independent Phase 0 midpoint audit

Reviewer: `/root/storage_data`; integration owner: `/root`.
Decision: **one unresolved MAJOR finding; do not promote the affected bundle write path until corrected and independently reviewed**. No new BLOCKER, MINOR or NIT findings. This is a bounded independent architecture, security and test-quality review, not final acceptance or a comprehensive security scan.

## Scope and immutable evidence

The complete plan was read before architecture work. This review consulted its narrow-waist requirement, project storage (§11), security (§15), validation (§16), Phase 0 roadmap and Gate C. The initial main baseline was `25b8249`; Sionna was reviewed in its frozen author tree and independently approved separately in `sionna-cpu-review.md`. Main advanced during this review; final observed HEAD was `5f0687983e139d9993b6d3620de1a77f56468c9e`. The hashes below identify the relevant actual source bytes; parent-owned status/ledger edits are outside this report.

Read-only inspection covered domain unit/time/response evidence boundaries, spatial grouping and bounded tile computation, point-survey configuration/state, project-bundle persistence, KismetDB and PCAPNG import boundaries, macOS stream validation and declared dependency enforcement. Existing tests and review scope statements informed adversarial probes. The reviewer did not approve their own `research/storage` implementation; its independent root review remains separate.

| File | SHA-256 |
|---|---|
| `crates/project-store/src/bundle.rs` | `6df28ec0d9635c8e34ad3543fd3f1005c9721ccf648d6fabd302811080a89934` |
| `crates/project-store/src/manifest.rs` | `f5b588a784a0812cb28d6df4b22ca8c20809cc6da6893928b92856348f561c11` |
| `crates/project-store/tests/bundle.rs` | `b60df74821fb9c2baf19891c3f0daf1fcdff03383f7fb87e1b9705c3e4ef4c5a` |
| `crates/spatial-analysis/src/model.rs` | `35d383d138d526300384c1514edc037584c350bba743fcc380d6fbbf8e88ecfd` |
| `crates/domain/src/units.rs` | `ec884a619b3569d5254285c8a0d38a2a3615add8d26d4a9e76370cfb6dd1a19e` |
| `crates/domain/src/observation/reception.rs` | `05f6f2375eb09f46c5492b3b064269a922da1a60566c0c6308f18cd0e08372a5` |
| `crates/kismet-adapter/src/database.rs` | `743bc5336340a70b34cbd6f677c17a18ab717be83ed90df251ada395ba18348a` |
| `crates/packet-import/src/pcapng.rs` | `3fd9d3e4856fa8128c2b1a264c08ca1c7cf1bf0a560d29a56e8caa5f4efb485f` |
| `collectors/macos/contract.py` | `528d05fd1c0017ea1bfb9fa8d962905117654ec815ba49b3f166b34e7e00b716` |
| `tools/architecture.py` | `9e45fb737c49e824e1785210c41aaeb55747a2a12370aa2799cafd4b9aee9529` |

## P0-MID-001 — MAJOR: SQLite trigger can silently discard a successful artifact registration

Affected source: `crates/project-store/src/bundle.rs`, `Bundle::open` (lines 152–179), `put_artifact` (lines 225–273). `configure` enables `trusted_schema=OFF`, which does not disable ordinary SQLite triggers. Open accepts a modified manifest table with an executable trigger. The write ignores the affected-row count, does not verify the stored result, publishes the proposed projection and commits successfully.

Reproduction against actual public Rust API:

1. Create a normal revision-0 bundle with `Bundle::create`, then close it.
2. Using SQLite, add:

```sql
CREATE TRIGGER ignore_write BEFORE UPDATE ON bundle_manifest
BEGIN SELECT RAISE(IGNORE); END;
```

3. Reopen with `Bundle::open(..., OpenMode::ReadWrite)`.
4. Register eight bytes `evidence` as a valid `MapSource` artifact using `put_artifact(..., 2)`.
5. Observe `Ok(hash)`, but `manifest().revision == 0` and `read_artifact(hash)` fails because the artifact is unregistered. The projection advertises the uncommitted registration; explicit recovery would remove that registration from the projection again.

This violates the successful-write contract and can lose the application's logical registration of newly acquired evidence. The blob remains orphaned on disk, so this is not a claim that its bytes were erased. It requires a supplied or modified project database; no privileged execution or concurrent filesystem race is required.

Recommended correction: validate the supported physical SQLite schema and reject unexpected executable schema objects before trusting it; constrain authorized operations; require exactly one affected manifest row; verify authoritative persisted revision/body inside the same transaction before projection publication/commit. Do not execute unknown schema logic merely to inspect a future-version bundle. Preserve explicitly supported future metadata inspection and version rejection semantics with tests. Add failure-first regression cases for ignored updates, trigger-altered data, views, malformed table layouts, and bounded query work. The implementer must receive independent review.

Existing 13 bundle tests check malformed manifests, revisions, symlinks, concurrent writers and projection failures, but do not supply an executable SQLite schema. They therefore do not disprove this plausible successful-write failure.

## Related resource-control evidence: already documented open boundary

A second bounded probe replaces the manifest table with a recursive view:

```sql
ALTER TABLE bundle_manifest RENAME TO hidden_manifest;
CREATE VIEW bundle_manifest AS
WITH RECURSIVE count(n) AS (
  VALUES(1) UNION ALL SELECT n+1 FROM count WHERE n<100000
)
SELECT singleton, revision, body FROM hidden_manifest
WHERE (SELECT max(n) FROM count)=100000;
```

`Bundle::open(..., ReadOnly)` accepts it and executes the supplied computation (32.223 ms in the recorded run, debug build). Source inspection finds no progress callback or cancellation/deadline budget in the bundle SQL path. Larger or nonterminating variants were deliberately not executed. Unlike KismetDB, bundle open does not require an ordinary manifest table. This supplies executable evidence for the hostile-import resource-control work already listed as open in `project-store-review.md`; it is not presented as an undisclosed completed capability or a separate newly discovered roadmap failure. Address it with the same schema-boundary correction where practical.

## Four bounded independent probes

Command, run from `.worktrees/sionna-review`:

```text
cargo test --offline --manifest-path .tools/midpoint-probes/Cargo.toml -- --nocapture
```

Result: **4 passed, 0 failed**, 0.09 seconds test execution (compilation excluded). These are reviewer reproduction/control assertions, not an approval of the defective behavior:

| Probe | Result |
|---|---|
| Trigger suppresses registration | Confirmed bad behavior: success with unchanged revision and unreadable registration |
| Supplied recursive view executes at open | Confirmed, bounded 100,000-iteration workload |
| Signed-zero coordinates at two locations | Passed: four input coordinates collapse into exactly two location groups, avoiding false support inflation |
| Source-response timing | Passed: reversed same-epoch receipt and mismatched epoch are rejected |

Original probe source and manifests are retained locally under `.worktrees/sionna-review/.tools/midpoint-probes/`; each bundle fixture has a unique process-ID name. No recursive deletion, implementation edit, optional escalated process probe or redundant full regression run was performed. The report contains sufficient SQL/API steps to reproduce the finding without relying on ignored files.

## Boundaries that survived inspection and review limits

Unit constructors canonicalize signed zero and reject nonfinite values. Observation response timing preserves the distinction between source API receipt and RF capture; the native-to-canonical integration is still explicitly open. Spatial input evidence-plane and uncertainty fields remain visible, unknown samples do not become measured zero, and distance-work limits/cancellation exist. Radius-based support is explicitly a numerical policy rather than a completed convex-hull/TIN model; do not relabel it as the later professional measured-heatmap implementation.

Kismet metadata stays foreign at the adapter boundary; its bounded private SQLite snapshot has ordinary-table checks, an authorizer and progress budget. PCAPNG framing preserves timestamp precision/remainder and raw bytes as borrowed evidence, without claiming completed 802.11 normalization. macOS advertises unknown capture time and conditional/unsupported capabilities. Declared Cargo dependency checks protect inward imports, but are not a proof that all standard-library side effects or semantic foreign-object leaks are absent. Sionna's limited CPU proof and pending material/GPU/full-runtime gates retain their separate reviewed scope.

Missing desktop UX, full canonical source normalization, product surveys, larger scientific models, storage Gate C and later roadmap capabilities are unfinished requirements already tracked, not newly introduced defects in claimed finished products. This bounded audit does not certify every parser input, every SQL engine feature, end-to-end usability, hard process memory/OS sandboxing or release safety. Re-run the targeted hostile-schema regressions after correction and obtain a nonauthor review before resolving P0-MID-001.
