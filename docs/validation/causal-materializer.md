# Canonical causal project materialization validation

The pure bridge is implemented in
`crates/causal-materializer/src/lib.rs`; it depends only on the canonical
domain, operation-log and materialization-identity crates.

Focused evidence includes:

- actual project and site rename effects on a real `Project`, with geometry
  retained by the domain aggregate;
- two independent equal-Lamport effects producing project-scoped V2 output;
- V1 project JSON preservation and V2 canonical serde round-trip;
- V2 typed calibration undo restoring `Unknown(NotMeasured)`;
- structurally valid forged V2 prior rejection against the causal baseline;
- resolution prior validation from the common causal subgraph and its
  unambiguous maximal field frontier;
- concurrent floor-evidence lock and calibration activation rejection as an
  explicit aggregate conflict;
- baseline/project identity and operation-ID collision admission;
- zero-time V2 decode rejection and immutable borrowed-input failure behavior.
- nested toggle replay, including redo effects receiving the redo operation's
  own identity;
- oversized operation-set rejection before conflict replay.
- a small accepted materialization and an actual 100-operation causal chain
  rejected by the cumulative copy-work boundary, with the baseline unchanged;
  the chain carries large site names so operation-induced data growth is part
  of the estimate.

Commands:

```text
cargo test -p kyberia-causal-materializer --locked --offline
cargo test -p kyberia-domain --locked --offline
cargo fmt --all -- --check
cargo clippy -p kyberia-causal-materializer -p kyberia-domain --all-targets --locked --offline -- -D warnings
```

This increment does not claim transactional project publication, signatures,
authorization, a persistent causal-state index, or full historical migration
of arbitrary V1 floor-evidence bindings. Those remain explicit follow-up
requirements.
