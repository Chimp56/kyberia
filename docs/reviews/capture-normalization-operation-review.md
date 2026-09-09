# Capture normalization operation review

Reviewer: root, independent of author Laplace. Source candidate
`91a0f60940eef6ea9af0a319663593582723b957`.

The extraction keeps one process supervision/decode/provenance path and moves
terminal/exit admission before the mapping callback. The opaque session binds
source process UUID, source clock UUID, native terminal and normalized capture
to one decoded stream. Neither accessor manufactures a host or canonical clock.
The decoder guarantees first hello/final complete records before constructing
its opaque stream. Existing survey persistence delegates to this operation.

Root independently ran `cargo test -p kyberia-capture-adapter -p
kyberia-observation-pipeline --locked --offline`: 56 passed, zero failed,
three ignored. Log: isolated normalization worktree
`.trash/normalization-root-review.log`. The added terminal/exit and command
mismatch tests assert that mapping was never called, rather than merely
checking the final error. Existing privacy/build/interface/limit and process
failure tests remain on the shared path.

## MAJOR finding and correction

The new unconditional `run_and_normalize` and `TerminalStatus` test imports
are referenced only by a Unix-gated test. They introduce unused imports on
Windows under the mandatory warnings-as-errors lint. Root requested and
inspected the additive `#[cfg(unix)]` guards on those two imports; no runtime
code or tests are removed by the correction. Frozen correction `e9bef50` is
approved: format and affected Clippy checks pass. Sources `91a0f60` + `e9bef50`
are integrated as `1fb8d12` + `ecf9b59`.

No other blocking correctness or dependency-direction finding was identified
in this bounded extraction. This does not validate Windows runtime, real
CoreWLAN scan authorization, in-flight crash recovery, identity allocation,
CLI orchestration or product survey UI. An author-observed timing-sensitive
malformed-process fixture failure was not reproduced by the root full affected
suite. It subsequently recurred during integrated testing: the malformed
fixture returned `Timeout` before decode under a one-second test budget.
Independently approved test correction `2b9b92a` (integration `73111b0`)
allows the adjacent parser tests' twenty-second startup margin while retaining
the dedicated hanging-child one-second timeout and elapsed-time assertions.
The correction's full package test suite and Clippy pass. A separate reviewer
also reported a descendant-drain timing failure; its exact evidence is being
requested and is not claimed resolved by the malformed-fixture correction.
