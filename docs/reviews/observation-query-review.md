# Indexed observation query review

Author: /root/operation_log_luna. Independent reviewer: /root.
Reviewed commit: 16ce58094faf80879c742db58ba79d7332b31f28; integrated as c2e210b.
Disposition: APPROVED. No unresolved BLOCKER or MAJOR findings.

Reviewed the complete query implementation, public exports, limits and disproof tests. The lookup uses the observation-ID index and verifies selected descriptors, artifact bytes, provenance and index ordinals. It rejects duplicate/missing IDs, future publication revisions and resource overflow. Cancellation after decode cannot return a partial selection. A 128-unrelated-corrupt-chunk fixture demonstrates that query decoding is confined to selected chunks. Metadata transactions do not span artifact decoding; selected immutable descriptors are rechecked, and this does not claim whole-bundle integrity.

Independent validation on the immutable candidate: `cargo test -p kyberia-project-store --test observation_chunks --locked --offline` passed 24 tests with one explicit benchmark ignored; focused Clippy with `-D warnings`, formatting and commit diff checks passed. Earlier review feedback added aggregate resource-limit, post-decode cancellation and future-publication-revision cases before approval.

Scope remains the storage lookup. Exact-BSSID selection, position association, selection-manifest construction and product heatmap workflow are not implemented by this query. The current API returns envelopes; a future composition receipt may need selected chunk hashes and a project revision to avoid querying whole inventories for provenance.

Integration on c2e210b: `KYBERIA_PYARROW_PYTHON=research/storage/.venv/bin/python cargo test -p kyberia-project-store --locked --offline` and `python3 tools/dev.py check` passed, including 179 Python tests (19 optional skips), locked source inventory, ledger and original fixtures.
