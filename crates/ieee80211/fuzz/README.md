# Parser coverage-guided QA

This isolated Cargo project drives the public Beacon/probe parser, arbitrary
canonical replay, and successful parse-to-canonical-to-replay equality under
three fixed bounded limit profiles. Input byte zero selects framing and the
limit profile; remaining bytes are the untrusted frame or canonical document.
Checked-in `0hex:` seeds are decoded only to bootstrap valid management frames.
Mutated non-hex input is exercised as raw bytes.

The exact tool pins are `nightly-2026-09-01`, cargo-fuzz 0.13.2 and the
dependency graph in `Cargo.lock`. `source-inventory.json` records licenses and
archive digests; verify it with:

```text
python3 crates/ieee80211/fuzz/check_source_inventory.py \
  --cargo-home .tools/cargo-home
```

From `crates/ieee80211`, after installing the pinned toolchain and cargo-fuzz
into worktree-local tool roots, the accepted bounded campaign command is:

```text
cargo fuzz run management_frame -- \
  -max_total_time=60 -max_len=11500 -timeout=5 \
  -rss_limit_mb=2048 -print_final_stats=1 -seed=20260914
```

The correction run and its exact counters are recorded in
`docs/validation/wifi-ie-fuzz-differential-run.json`. Generated corpus and
coverage files are build evidence, not source; retain them under the worktree's
ignored `.trash/test-runs/` instead of deleting them. Keep the six reviewed
seeds checked in. The seed contract proves that these reach canonical decode,
raw parse, and CRC-valid FCS parse paths:

```text
cargo test --manifest-path crates/ieee80211/fuzz/Cargo.toml --locked --offline
```

Inspect any retained flat corpus with the deterministic digest and semantic
reachability tools:

```text
python3 crates/ieee80211/fuzz/inspect_corpus.py CORPUS_DIRECTORY
cargo run --manifest-path crates/ieee80211/fuzz/Cargo.toml \
  --example corpus_reachability --locked --offline -- CORPUS_DIRECTORY
```

The inspector hashes sorted UTF-8 basenames, NUL, decimal byte lengths, NUL,
and exact contents. It rejects symlinks and non-file entries. A passing bounded
campaign is regression evidence, not proof that arbitrary future inputs or
platforms are defect-free.
