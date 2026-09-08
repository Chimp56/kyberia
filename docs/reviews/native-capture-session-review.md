# Native capture session review

Candidate: `652c6c7`. Author: /root/operation_log_luna.
Independent reviewer: /root. Disposition: REQUEST_CHANGES.

Earlier draft review required cancellable pipe supervision, direct process
group signalling, cleanup failure reporting, rejection of forced partial
drains, and an escaped-session descendant test. The candidate implements
poll-based readers and these corrections. The author also executed a real
redacted CoreWLAN capability probe through the Rust boundary; this is not
authorized RF scan evidence.

## MAJOR: typed privacy request not bound to producer declaration

`run_and_persist` compares decoded command name and timeout with the typed
request, but does not compare the requested identifier policy. A caller may
request `ScanOptions` with identifiers disabled, receive a valid stream whose
hello declares included identifiers, and provide an owned-infrastructure
mapping context. Normalization can then succeed and persist identifiers that
the typed command did not request. The current privacy test only rejects a
redacted mapping context against an included stream; it does not cover this
request/response mismatch.

Require command policy checks before mapping or persistence. Probe must be
redacted; scan privacy must match the explicit flag. Add the included-context
regression and assert no durable output on mismatch. Bind observation count
to the requested scan limit, and check requested interface against reported
scan sources where the protocol provides that evidence.

## MINOR: unsupported platform is not a distinct outcome

The non-Unix implementation returns `ProcessIo`, whose message describes an
I/O failure. Add a distinct unsupported-platform outcome and corresponding
documentation. A platform capability boundary should not make an unsupported
operation indistinguishable from a broken supported collector.

The candidate remains isolated pending corrections and independent review.
