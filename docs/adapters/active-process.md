# Bounded iperf3 process research

This is a Phase 0 Gate D research adapter. Four actual direct iperf3 loopback runs and 20 deterministic parser/process tests have passed. The corrected final wrapper passes eight actual loopback acceptance checks on macOS ARM64 after explicitly authorized socket access; [independent correction review](../reviews/active-luna-review.md) is approved. No ACT/ACTB product feature or full Gate D completion is claimed.

## Scope and contract

`research/active/contract.py` defines frozen typed request/measurement records and a bounded JSON parser. `process.py` launches the trusted local binary; `cli.py` exposes an explicit local research entrypoint. There are no Python package dependencies and no dependencies on Sionna, CoreWLAN, Rust domain internals or native iperf objects. The outer result and request both use schema version string `"1"`; adapter version is `0.1.0`. This is not the final canonical active-domain wire schema.

Requests require all fields and reject unknown/duplicate fields. A request example is retained as [request.json](../../research/active/request.json). Literal IP targets prevent DNS rebinding or command-option injection. Topology must be explicitly `loopback`, `lan` or `remote`; it is an assertion, not automatic network diagnosis. Only `127.0.0.1` with `owned_loopback` authorization can execute. Non-loopback, IPv6, parallel streams, QUIC and bidirectional tests return unsupported before process creation. No authentication is implemented, and the proof binary intentionally excludes OpenSSL. No arbitrary command payload or server mode is accepted from a request.

Requests validate 1–2 seconds, 1–4 streams and 1,000–10,000,000 offered bits/sec aggregate per direction, divisible by the stream count. Currently only one stream executes. The actual initial runs used one second and 2 Mbps; other bounded duration/rate choices have not had a live wrapper acceptance run. TCP pacing is a requested average, not a hard instantaneous traffic cap. UDP uses an explicit 1,200-byte payload. Warm-up is zero. Fixed-byte tests and continuous probes remain unsupported/unimplemented.

Successful measurements retain both endpoint summaries: source duration in seconds, bytes, bits/sec and explicitly derived decimal Mbps (`bits/sec / 1,000,000`). Upload means client→server; download means server→client. The `sum_sent`/`sum_received` keys identify measurement role; the requested reverse mode identifies its endpoint. TCP 3.20 uses the local sender flag for both summaries, whereas UDP marks sender/receiver separately. UDP preserves raw source counters separately from interpreted evidence. Sender `packets` means sent datagrams; receiver `packets` means highest sequence seen, while `received_datagrams` is bytes divided by the verified 1,200-byte payload size. The receiver percentage uses the sender packet count as its explicit denominator. It is labeled `iperf_sequence_gap_estimate`: trailing loss is unobserved and duplicates can reduce the gap count. It is not exact end-to-end loss. Sender loss/jitter placeholders become `null`; receiver loss is unknown without any arrival, and jitter is unknown with fewer than two arrivals. The original placeholder numbers remain in `source_*` fields. Reordering distributions and packet-level latency are not normalized by this increment.

Source JSON must identify iperf 3.20 and match request target, port, protocol, duration, stream count, rate, UDP payload size, reverse/bidirectional flags, omit and test type. Connection hosts must be bounded strings; integer IPv4 representations are rejected before address parsing. Both end summaries must exist and have consistent positive durations; byte counters are nonnegative exact integers no larger than `2^53−1`, because upstream cJSON uses binary64. Summary rate must agree with `8 * bytes / seconds` within `1e-5` relative or `0.01` bits/sec absolute tolerance. Supported one-/two-second duration agreement permits 25% scheduling tolerance (plus 0.1 seconds absolute); this is validation against incomplete output, not a precision claim. Additive unknown upstream fields are ignored semantically but still scanned for structural/nonfinite/duplicate hazards.

The process window records host monotonic start/end in an explicit unique epoch and a host UTC reading taken before execution. It describes client-process execution, not RF capture, exact packet timing or a hardware clock. The iperf start Unix milliseconds are retained separately; synchronization is unknown. Results keep target request attribution and declare `wifi_attribution: not_established` for all states.

## Lifecycle and bounds

`run()` performs a capped version probe before the client. Version probe timeout is at most two seconds; client timeout defaults to eight seconds and is constrained to 0.05–10 seconds. These are separate deadlines, so total elapsed time can include both plus process reaping. The test duration itself is at most two seconds. Commands use fixed argv with `shell=False`, no stdin, a minimal environment and a new POSIX process group.

Client stdout is limited to 256 KiB, stderr to 16 KiB and version output to 8 KiB. JSON depth is limited to 32 and visited values to 20,000; requests are limited to 4 KiB. Cancellation, wall timeout or oversized output kills the owned process group and reaps the direct child. Normal direct-child exit also terminates remaining group descendants, including descendants that have closed standard streams. Exit observation uses non-reaping Linux `waitid` or BSD/macOS `kqueue`, retaining the direct child until cleanup. A Darwin zombie-only group can return `EPERM`: after reaping, a non-mutating signal-zero probe must report `ESRCH` to establish absence. No destructive signal is sent after reaping, avoiding group-ID reuse hazards. Other cleanup failures produce a structured `process_error` and no measurement; pipe handles close on every path. Descendants are reaped by their OS parent, which is outside this supervisor’s direct wait ownership. A child that exits while a descendant holds a pipe cannot keep the supervisor waiting beyond the deadline. POSIX platforms only are implemented; macOS ARM64 is the observed build platform.

Terminal states include `completed`, `unsupported`, `unavailable`, `process_error`, `invalid_output`, `cancelled`, `timeout` and `output_limit`. Every non-completed state has `measurement: null`; a command failure never becomes zero Mbps. Missing/malformed requests yield the CLI's `invalid_request` state. Source failures and version-probe failures are distinct reasons. The selected executable is trusted operator configuration, not hostile input. The adapter does not provide an OS sandbox, authenticated-agent service, concurrency scheduler or hard memory/CPU quota.

## Build and reproduction

The source pin is the official ESnet 3.20 archive, SHA-256 `3acc572d1ecca4e0b20359c7bf0132ddc80d982efeee20c86f6726a9a6094388`. [ESnet's CLI documentation](https://software.es.net/iperf/invoking.html) describes the direction and bitrate options; pinned `src/iperf_api.c` is the source of truth for the observed JSON semantics. The proof intentionally tests this exact release rather than assuming current documentation means every newer release is compatible. See the [source/build/license inventory](../licenses/active-research-sources.json).

The author downloaded and verified the archive, validated that archive members were ordinary files/directories under `iperf-3.20` without traversal, then extracted into ignored `.tools`. Existing source/build directories were preserved. Reproduction in a fresh ignored directory:

```sh
curl -fL https://downloads.es.net/pub/iperf/iperf-3.20.tar.gz -o .tools/downloads/iperf-3.20.tar.gz
shasum -a 256 .tools/downloads/iperf-3.20.tar.gz
```

After verifying the hash and safely extracting, run from `.tools/iperf-3.20` (substitute the actual absolute worktree path):

```sh
./configure --disable-shared --without-openssl --prefix=/absolute/worktree/.tools/iperf-install
make -j4
make check
make install
```

Observed build: Apple Clang 21.0.0, macOS ARM64, only `/usr/lib/libSystem.B.dylib` dynamically linked; libiperf/cJSON are compiled into the executable. Configure's initial `sysctl` probe was sandbox-denied, so configuration was rerun outside the sandbox and the binary rebuilt and installed. Deprecation and cJSON integer-to-double warnings were observed in the initial compile; counter bounds account for cJSON's numeric representation. Upstream `make check` subsequently passed 5/5. `t_auth` passes its no-OpenSSL branch, not real authentication.

From the worktree root:

```sh
python3 -m unittest tests.test_active_process -v
python3 research/active/cli.py --binary .tools/iperf-install/bin/iperf3 --request research/active/request.json
python3 research/active/acceptance.py --binary .tools/iperf-install/bin/iperf3 --output research/active/evidence/loopback-macos-arm64.json
```

The CLI expects an explicitly supplied owned loopback server. The acceptance script launches only its own one-off servers bound to `127.0.0.1`, compares remote byte summaries with server counters, then attempts real cancellation, timeout, recovery and connection-refused cases. It records unsupported parallel cases without executing them. The final command passed eight checks after the required socket escalation was approved. Earlier sandbox denial, interrupted approval and rejected false-success evidence are distinct from this corrected execution. The helper propagates thread and cleanup errors before it can write a passing report.

## Evidence

Four [raw client fixtures](../../research/active/fixtures/README.md) came from actual iperf3 processes at 2 Mbps, one second and one stream: TCP upload/download and UDP upload/download. The [initial report](../../research/active/evidence/initial-loopback-macos-arm64.json) preserves source output hashes, normalized data, CPU summaries and matching server byte counters. These were direct initial process invocations before final-wrapper acceptance. The initial executable hash was not captured before the subsequent rebuild; the current installed hash is separate evidence, not retroactive proof of byte identity.

Actual server output contains one identical duplicate `start.target_bitrate`. An evidence-only reader permits exactly that known location/count/value and rejects every other duplicate; client ingestion remains fully strict. Endpoint summary comparison is not independent packet capture. No shaped-loss, Ethernet bottleneck, Wi-Fi, LAN/Internet, authentication or RF attribution test was performed.

The 20 ordinary tests include malformed/truncated JSON, duplicate/nonfinite/overflow/structural bounds, request/source mismatches, attribution, units, counter coherence, unsupported modes, and real subprocess lifecycle exercised with explicitly fake children. Fake-child cancellation, timeout, crash, output flooding and inherited pipes prove supervisor behavior only. Separate actual wrapper evidence validates iperf cancellation/timeout/recovery in the bounded loopback mode. Neither test set establishes a usable active-survey feature. All temporary outputs remain ignored and are not recursively cleaned.

The lifecycle suite is explicitly POSIX-only; parser tests run on every host and a portable contract test verifies the non-POSIX unsupported state. Root review reproduced both numeric-host coercion and a surviving background descendant before correction. Both retained regressions failed against the original source and pass with the correction; the independent Luna review verifies the final corrections.

## UDP review corrections

Synthetic parser regressions cover 210 sent datagrams, highest received sequence 200, ten internal gaps and 190 received datagrams: the source reports 10/210, excluding ten trailing losses. Both upload and download are tested. Duplicate arrivals may exceed the sent count; they are not falsely rejected as impossible. Zero/one-arrival timing, forged payload sizes and contradictory counters are checked. These are independently constructed parser probes, not controlled-loss runtime evidence.

Pinned source references: `src/iperf_udp.c:125–170` tracks sequence gaps and duplicate/reordered arrivals; `src/iperf_api.c:4167` selects the loss denominator, and `src/iperf_api.c:4430–4444` emits percentages and the separate summaries. `receiver_total_packets` aggregates the highest-sequence counter for this single-stream mode; it is not the received datagram count.

A subsequent actual wrapper run exposed a server-thread cleanup exception that the acceptance helper failed to propagate. Its apparent eight-check success is rejected in `research/active/evidence/loopback-rejected-thread-error.json`. The corrected helper fails on thread or cleanup errors; the corrected eight-check run in `research/active/evidence/loopback-macos-arm64.json` passes with exact executable hashes and no cleanup errors. This resolves the bounded macOS wrapper acceptance, while shaped-loss, other OS execution and product integration remain open.

The corrected live run covers TCP/UDP upload/download at one second and 4 Mbps, real cancellation, timeout, recovery, and a refused connection. All executing results identify the current binary SHA-256 `251a081d992ee0e9d69d6f585b332633b49feb8ed502f7febb2ad59d9812df12`. Linux waitid and other POSIX paths require platform execution; macOS kqueue is the actual observed path. Source-guided independent cleanup diagnostics and the review record are retained separately.
