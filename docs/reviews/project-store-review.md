# Independent project store and CLI integration review

Reviewer: `/root/qa_spec_audit`. Author/integrator: `/root`.
Decision: **APPROVED for the selected initial transactional container and CLI**.
No unresolved BLOCKER or MAJOR finding remains in this reviewed increment.

The reviewer inspected `crates/project-store`, `apps/cli`, workspace settings,
bundle documentation, ADR-0001/0002, source inventory scope, and the earlier
independent review at `.worktrees/domain/docs/architecture/project-store-review.md`.
Relevant plan requirements: §§11, 14, 15, 19 iteration 1, 20 Gate C, and 23.
Primary source was not modified. Tests and standalone probes wrote only build
outputs and retained temporary projects.

The primary branch HEAD during final hash capture was
`39d670a1c45ef5ba82794e7f31c0e3e0af5ea9ef`; the reviewed new storage/workspace
files were working-tree changes. The following hashes identify the actual bytes
reviewed, without attributing uncommitted files to that baseline commit.

| File | SHA-256 |
|---|---|
| `crates/project-store/Cargo.toml` | `e22619ba9dfd4476bb3c144c244382fa953b83853739d83029094a50b472ec32` |
| `crates/project-store/src/lib.rs` | `a19ef2212bf046b150a998a8370e5dc9430c35d9f2fda1943099fa9e01ef91ec` |
| `crates/project-store/src/manifest.rs` | `f5b588a784a0812cb28d6df4b22ca8c20809cc6da6893928b92856348f561c11` |
| `crates/project-store/src/bundle.rs` | `6df28ec0d9635c8e34ad3543fd3f1005c9721ccf648d6fabd302811080a89934` |
| `crates/project-store/tests/bundle.rs` | `b60df74821fb9c2baf19891c3f0daf1fcdff03383f7fb87e1b9705c3e4ef4c5a` |
| `apps/cli/Cargo.toml` | `7e0476d4e0bc1b10ee1491faa1687e36734ee8a4414d46c7438b3f3b491dde29` |
| `apps/cli/src/main.rs` | `aed0212ec075abb66a34846b2bbcaff68770072ec58c8b30ddf8a3daf1890226` |
| `apps/cli/tests/project_workflow.rs` | `6049012139e852a81b0c4edca877b635497ea85a51cca25b166405d971110f66` |
| `docs/architecture/project-bundle.md` | `4c89bd5e919b2944ec6fa0a049bed746af769a201321acd95152f96a8b76de66` |
| `Cargo.toml` | `925ecf29dc8629ff45c3d2f9cfb61ae415db74588a3d8ea0ba447a85674ab22e` |

## Earlier findings independently verified

The previous reviewer identified three MAJOR defects; the final code and
runtime regressions substantiate their correction:

1. Stale writer handles revalidate manifest body/revision/SQLite schema and
   supported required features under the immediate transaction before canonical
   mutation. Tests corrupt revision and schema and simulate a future schema.
   An additional reviewer-compiled probe added an unknown required feature
   after opening a writer: both write and recovery were refused, with committed
   manifest preserved exactly.
2. Projection publication now occurs within the serialized write transaction.
   The projection-failure test checks both immediate and reopened committed
   state after a publication error. An independent probe wrote an ahead JSON
   projection, verified its detection, recovered from SQLite, and reopened the
   unchanged canonical revision. Orphan blobs and an ahead/stale projection
   remain documented recoverable consequences of an interrupted write.
3. Verification loads/validates the manifest once, then hashes registered
   artifacts directly. It no longer reparses the full manifest per artifact.
   This is static algorithmic confirmation, not a claimed large-project
   performance benchmark.

The earlier CLI MINOR issue is also resolved: after repairing a broken
projection, recovery reports remaining blob corruption and exits 1. It does
not equate successful projection publication with complete integrity recovery.

## Independent execution

```text
cargo test --workspace --offline --locked
PASS: 13 store tests; 2 executable CLI tests; 18 domain tests;
      7 domain compile-fail doctests

cargo fmt --all -- --check
PASS

cargo clippy --workspace --all-targets --offline --locked -- -D warnings
PASS
```

The additional retained Rust probe passed stale-required-feature refusal and
ahead-projection recovery. Existing tests exercise create/reopen/identity,
duplicate content, corruption/missing blobs, schema compatibility, malformed
declarations, read-only mutation refusal, static symlink rejection, sequential
independent writers, and CLI status behavior. No recursive deletion was used.

## Scope limits and follow-up debt

The documented MINOR limitations remain explicit: wall-clock rollback rejects
writes until the application supplies a valid modification time, and identical
bytes with different provenance/kind cannot yet have independent usage records.
Resolve these before capture persistence and multi-source imports depend on
the adapter. Raw acquisition timestamps must remain unchanged.

This initial directory API assumes no concurrent malicious filesystem
replacement. Path checks are not descriptor-relative race-proof confinement;
SQLite sidecar trust, hostile import resource control, encrypted containers,
Windows directory durability, cancellation, and crash/power-loss testing on
other platforms remain open. Verification during concurrent writes is not a
certified point-in-time project snapshot. These limits do not imply a complete
untrusted-import or release security boundary.

SQLite owns one committed typed manifest; generic artifact blobs are not a
completed observation database, Parquet pipeline, geometry model, report
manifest engine, or desktop workflow. No prior production schema existed, so
there is no fabricated migration fixture. Compatible future metadata can be
inspected read-only; a complete future migration strategy remains work.

ADRs correctly accept canonical boundaries/toolchain direction while leaving
renderer, storage split, geometry, optimizer, and external runtime gates open.
The reviewed workspace is the initial Cargo foundation; pnpm/Tauri/React
application delivery remains unfinished. This approval does **not** complete
storage Gate C, Phase 0, iteration 1's entire product scope, or the full plan.
