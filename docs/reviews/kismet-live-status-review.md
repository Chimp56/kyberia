# Kismet live status review

Follow-up MAJOR, independently reproduced on proposed DNS correction
`cf9755376851f32f4b6dc97a75cf5314ef91170d`: a slow TLS handshake still
overruns the deadline. The isolated original fixture accepts a local TLS
connection, reads ClientHello, writes record header `16 03 03 00 10`, and
trickles sixteen zero bytes at 40 ms intervals. A 100 ms poll returned
`DeadlineExceeded` only after **744.287584 ms**, failing the deliberately
generous 350 ms elapsed-time assertion. Command:
`cargo test -p kyberia-kismet-adapter --test tls_deadline_review --locked --offline`
in the isolated `test/kismet-timeout-review` worktree. The failing disproof
is not integrated into main.

Pinned ureq `src/rtls.rs:108` performs `complete_io` over the TCP stream;
`src/stream.rs:433` sets a socket timeout once. Repeated handshake reads can
therefore remain below the inactivity timeout while exceeding total elapsed
time. Require a mature transport or process boundary that actually cancels
the handshake, retain certificate/hostname verification, and add this
regression plus an elapsed-time assertion for the existing slow-body test.
The explicit-address correction alone does not close transport review.

Current disposition: REQUEST_CHANGES for hostname-resolution deadlines.
Follow-up inspection of pinned ureq 2.12.1 `src/agent.rs:471` and
`src/stream.rs:364` confirms system DNS resolution cannot be interrupted by
the request timeout. The existing slow-body tests exercise socket deadlines,
not this stage. The author is adding explicit bounded transport-address
binding while preserving HTTPS hostname certificate verification; implicit
unbounded resolution must not remain inside the bounded poll. DNS acquisition
needs its own cancellable integration. Earlier approval below is historical
and superseded pending this correction and independent regression review.

Author: /root/channel_coupling_review_luna. Independent reviewer: /root.
Reviewed candidate: `8e6f109f601647a3ea22895e15ef7045af2f5e96`, including
`1de6691` and `9155b5a`; integrated through `1f7a943`.
Disposition: APPROVED for the bounded read-only adapter. No unresolved
BLOCKER or MAJOR findings in this scope.

Review covered endpoint and secret handling, producer admission, selected
upstream field semantics, malformed JSON and resource limits, deadlines,
retry behavior, unknown values, and separation from canonical observations.
Earlier review findings required final-response cancellation/deadline checks,
an exact source/version admission tuple instead of accepting arbitrary date
versions, and HTTPS for remote endpoints. All were corrected before approval.
The local plaintext exception admits literal loopback addresses only; redirects
are disabled. Authentication failures do not become empty inventory success.

Independent validation on the immutable candidate:
`cargo test -p kyberia-kismet-adapter --locked --offline` passed 14 unit,
16 database, and 5 HTTP transport tests, with one explicit database benchmark
ignored. The transport suite exercised real local sockets, including a body
that stalled past the shared deadline. Socket tests required execution outside
the filesystem sandbox; the earlier sandbox bind denial was not a test pass.

This approval does not validate a real Kismet server, remote TLS deployment,
live packet acquisition, WebSockets, source dwell/drop accounting, clock
synchronization, or the complete OSS-001 runtime gate. Socket cancellation
is observed at bounded transport return/checkpoints, not immediate syscall
interruption. The API version/source fields are compatibility evidence from a
trusted configured endpoint, not cryptographic binary attestation. The fixture
acceptance tuple must be reproduced by a controlled pinned server build before
claiming live-server compatibility.

Integration on `1f7a943`: `python3 tools/dev.py check` passed formatting,
workspace Clippy/typecheck, 398 Rust tests including doctests (8 explicit
benchmarks ignored), 179 Python tests (19 optional skips), architecture and
181-package source verification. Its ledger step caught two stale source-ledger
hashes introduced by the reviewed dependency update. After refreshing those
references, `python3 tools/ledger.py check` and
`python3 tools/validation/fixtures.py check` both passed. No failing runtime
test was skipped to integrate this increment.

## Reqwest correction: focused independent reproduction

Candidate `f84e13d7ab8af8b87941c87cccd381543fe30328` replaces ureq with
reqwest 0.12.28. Root independently executed
`cargo test -p kyberia-kismet-adapter --test tls_deadline --locked --offline -- --nocapture`:
both tests passed. The original incomplete-record TLS trickle reproducer now
returns within its 350 ms acceptance bound for a 100 ms request deadline;
explicit-address TLS still sends the original hostname in ClientHello.
This closes the specific reproduced handshake overrun on this host, but is
not final approval of the transport change. Full adapter, dependency and
integration review remain pending; real-server validation remains open.
