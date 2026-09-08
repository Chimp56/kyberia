# Native capture session

`kyberia-observation-pipeline::process` is the outward process/session
composition boundary for the Phase 0 macOS collector. It takes an
operator-selected `TrustedCollector`, one of the closed `Probe` or `Scan`
commands, an explicit identity mapping callback and the existing canonical
pipeline request. The adapter and survey crates remain inward of this
boundary; no process, SQLite, packet, or platform type enters the canonical
observation contract.

## Trust and command construction

`TrustedCollector::new` accepts only an absolute, regular, non-symlink,
executable file and binds it to an expected source-build `ContentHash`.
`ProbeOptions` and `ScanOptions` validate their bounded values before the
process starts. The supervisor constructs every argument from those typed
values. It does not accept a shell string, arbitrary argument vector,
environment-selected executable, inherited environment, or plugin capability.
The child receives a cleared environment and absolute executable/fixture paths.
The test fixtures use
small shell wrappers only to exercise real process supervision; the production
path invokes the trusted file directly.

The collector's first hello record is decoded before any observation is
accepted. Its command name, declared timeout and source-build hash must equal
the typed request and trusted executable contract. Adapter provenance checks
then validate the source, build, protocol, clock epoch and terminal record.
The mapping callback supplies the canonical process-session, source and
observation identities. The session layer never derives an identity, pose,
capture clock or dwell value from process output.

## Bounded process lifecycle

On Unix, each child starts in a fresh process group owned by the session. The
stdout and stderr readers use `poll(2)` with a 20 ms deadline and check a
shared stop flag on every poll. Each stream has an independent byte limit
(`MAX_STREAM_BYTES` for canonical stdout and `MAX_STDERR_BYTES` for
diagnostics). The supervising loop checks cancellation, stream limits and the
typed deadline while polling `try_wait`.

Timeout, cancellation, output overflow and I/O failure set the reader stop
flag, signal the owned process group through the Rust `rustix` process API,
and reap the direct child with `wait`. A child that exits while a descendant
still holds a pipe cannot hold the session open: the readers continue to
poll, receive the stop flag and return; the supervisor accepts their channel
result only within bounded drain windows, then joins a reader whose result was
received. It never detaches a blocked reader as a successful capture. If a
pipe needed forced draining, the session fails closed even when the bytes
already collected happen to decode as a complete stream; cleanup or reader
failure is reported as a process-cleanup/I/O error. A descendant that calls `setsid` is outside the
owned group and is therefore not claimed as killable by this contract; the
escaped-group fixture is short-lived and proves that its inherited pipe does
not make the reader or session block. A future platform adapter must provide
the same bounded read and process-ownership guarantees before enabling this
module outside Unix.

The terminal status and process exit code are a closed mapping: success is
`0`, partial is `2`, permission required is `77`, unsupported/unavailable is
`69`, error is `70`, timeout is `124`, and cancelled is `130` or `143`. Any
other pairing fails before normalization or durable publication. Permission
is never requested implicitly; a permission-required terminal is recorded as
evidence and follows the normal terminal publication path when its exit code
agrees.

## Canonical publication

After process supervision, decode and normalization produce the reviewed
`NormalizedCapture`. It is converted once into a bounded
`ReceivedObservationBatch`, which checks source-reference hash/length closure,
observation identity uniqueness, raw-retention policy and the typed capture
manifest. `ingest` validates every receipt association before invoking the
existing project-store adapter. The durable publication order and recoverable
partial progress semantics are defined by
[`native-observation-pipeline.md`](native-observation-pipeline.md): no survey
association may be committed for an observation absent from normalized
storage, and an exact retry can converge after an intermediate publication
failure.

Empty, partial, permission-required, unsupported and error captures preserve
their terminal and capability evidence. An empty capture has no fabricated
observation, chunk or capture time; it can still publish its terminal
manifest and unchanged survey snapshot according to the pipeline contract.
Raw identifiers remain governed by the adapter's explicit privacy policy.

## Scope and open integration gates

This increment supplies process/session supervision and its composition into
the reviewed pipeline. It does not implement the real CoreWLAN transport,
authorization UI, capture scheduler, CLI command wiring, hardware-specific
identity mapping, or strict point completion from measured pose/time. Those
remain platform/product integration gates. The process API is intentionally
an outward composition module until those integrations can provide the
canonical mapping and consent boundary.
