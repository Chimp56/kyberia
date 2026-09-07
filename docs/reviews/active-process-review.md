# Active process proof: independent root review

Status: Original findings resolved by root-authored corrections and independently approved in the [final Luna xhigh review](active-luna-review.md). The historical findings below review the original frozen `feat/active-process-proof` worktree, not a completed active-survey feature. `/root` did not author that original implementation and did not approve its own corrections.

Reviewed source hashes: `research/active/process.py` `1c33e9959ce10723fe15d049499d99a141e275453c17ef4e87543f882eb41350`; `research/active/contract.py` `83506e3163d3ef7ea8759c0db9041e2f62a411bbe6c69be0819db89ca1c02607`. The complete 16-file author freeze is retained in the author's ignored `.tools/active-freeze.json`.

## ACT-R001 — MAJOR: successful parent exit leaves a descendant running

`execute()` waits while pipes remain registered or the direct child is alive. Its final cleanup stops the process group only if that direct child remains alive. A descendant that closes its inherited standard streams survives successful completion of the direct child. This contradicts the supervisor's bounded process-lifecycle claim and can leave background work after a reported terminal state.

Independent reproduction on macOS ARM64: launch an original Python test child through `execute(..., timeout_s=0.3)`. That child forks; the descendant closes descriptors 0/1/2 and sleeps for 30 seconds. The parent writes the returned descendant PID to an owned temporary file and exits with `os._exit(0)`. `execute()` returns `completed`; immediately probing the recorded PID with `os.kill(pid, 0)` succeeds. The reviewer then kills only that created descendant. No live network test or destructive filesystem cleanup was used.

Required correction: ensure process-group cleanup also covers natural direct-child exit and descendants without inherited pipes. Add a lifecycle regression proving the owned descendant terminates after the supervisor returns. Keep exact limits and unavoidable OS/reaping limitations explicit; do not substitute a timeout success state.

## ACT-R002 — MAJOR: numeric IP payload escapes the text contract

`parse_result()` passes connection host values directly to `ipaddress.ip_address`, which accepts integer IPv4 representations. Mutating the actual TCP upload fixture's `start.connected[0].remote_host` to integer `2130706433` is accepted and retained as an integer in the normalized connection record. The declared JSON host contract is text; accepting another representation silently changes the schema.

Required correction: validate both local and remote connection hosts as bounded strings before parsing their IP semantics. Add fixtures for integers, booleans, null, arrays and objects; retain successful actual string-valued fixtures. Verify all consumed source fields have explicit types rather than relying on permissive library coercion.

## Scope and evidence limits

Integration update: corrected string-only host validation, descendant cleanup and macOS zombie-group handling pass independent probes and 20 tests. The UDP source-denominator and placeholder interpretation defect was corrected following Luna's additional review; the wrapper now propagates thread/cleanup errors. Eight actual corrected wrapper checks pass. The earlier apparent success is retained as explicitly rejected evidence. Exact final hashes and remaining scope limits are in the linked final review.

The documentation correctly distinguishes four initial actual iperf3 runs from the final wrapper's unexecuted live acceptance. The initial executable hash remains unknown after its rebuild; the installed binary hash is separate. Parallel/nonloopback/QUIC/bidirectional execution remains unsupported. These honest limitations do not excuse the two reproduced implementation defects. No approval is given until they are corrected and independently retested.
