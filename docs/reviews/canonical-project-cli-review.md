# Canonical project CLI review

Source candidate: `57f8129`. Reviewer: root, independent of CLI author Laplace.
The four real CLI integration tests passed independently.

## MAJOR: mixed committed revisions

`query` combined three independently committed read transactions: manifest,
immutable baseline, and current publication. A concurrent registration or
publication could cause an internally inconsistent report, including a current
publication newer than the reported bundle revision. Source inspection of the
three storage getters confirms each transaction ends before the next begins.

The correction adds `Bundle::canonical_project_snapshot`, returning the manifest,
baseline and current project from one transaction. Current-publication validation
is shared with the existing getter; immutable artifact and receipt checks remain
in place. The CLI consumes this snapshot instead of separate getters.

The deterministic storage regression holds a read snapshot while a second handle
attempts publication. Under the configured DELETE journal mode the writer cannot
commit; the reader observes the complete prior state. After the reader completes,
publication succeeds and the next snapshot contains the matching receipt and
manifest revision. Fixtures are retained under `.trash/test-runs/`.

Validation of the correction:

- `cargo test -p kyberia-project-store -p kyberia-cli --locked --offline`: PASS.
- `cargo clippy -p kyberia-project-store -p kyberia-cli --all-targets --locked --offline -- -D warnings`: PASS.
- `cargo fmt --all`: applied.
- `tools/architecture.py`, `tools/source_inventory.py check`: PASS.

Independent reviewer `/root/offset_review_luna` (Beauvoir) approved the complete
candidate `0ab4bc7` atop `57f8129` with no findings. The reviewer reran 155
affected tests (one ignored), the focused concurrency regression, Clippy,
format, architecture and source-inventory checks. Logs are retained in the
candidate worktree under `.trash/test-runs/canonical-project-cli-review-0ab4bc7`.

The MAJOR mixed-snapshot finding is resolved. SQLite does not freeze external
artifact files; bounded reads and exact hashes independently validate those
files and reject corruption. This approval does not claim full project-management
UX acceptance or native Windows execution.
