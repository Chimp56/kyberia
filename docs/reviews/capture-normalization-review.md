# Independent review: native capture normalization

Verdict: **APPROVED for the bounded macOS v1 canonical normalization adapter after CAP-R001 correction**. No unresolved BLOCKER or MAJOR findings. This is not native transport supervision, authorized field capture, strict point-survey admission, channel inspection UI or full capture Gate A acceptance.

Reviewer: primary integration engineer, independent of the author; inspected the native Swift producer, canonical contracts, decoder, normalizer and tests in isolated `review/capture-normalization`.

## Findings

| Severity | ID | Finding | Resolution |
|---|---|---|---|
| MAJOR | CAP-R001 | One CoreWLAN call could claim conflicting API completion times across its results; an API end could precede the emitted start event even though the native call begins afterward. | Per-scan state retains the start event receipt and the first API completion. Subsequent results require the same completion, and completion cannot precede the start event. Both independently reproduced failing cases now pass rejection tests; equal clocks and UTC clock steps remain valid. |

The author independently identified the `time` 0.3.44 advisory before integration and updated the pin to patched 0.3.55. The adapter only invokes a narrowly validated RFC3339 path, but no advisory exception is needed. Registry inventory and full dependency auditing are separate integration checks.

## Independent validation

`cargo test -p kyberia-capture-adapter --offline --locked` against the corrected source: **19 author tests plus five independent review tests PASS**; two explicit support commands remain ignored. The tests use original synthetic inputs, never undisclosed real RF captures.

Two independent chronology regressions initially failed as expected: changing the API end to 2901 before the start-event receipt 3000, and adding a second observation under the same scan with a distinct end. Both reject after correction. Additional independent checks accept a backwards UTC step with valid monotonic ordering, verify exact signed-nanosecond conversion for the Unix epoch, one millisecond before it and leap-day 2000, reject impossible dates, and reject nested duplicate keys. These five reviewer tests are retained separately in `crates/capture-adapter/tests/reviewer_probes.rs` in the review worktree; they are not claimed as five main-branch tests.

The reviewed author tests cover all seven synthetic terminal fixtures, required source/observation mapping, explicit redaction, binary SSID bytes, true zero and full u64 monotonic values, source-version unknowns, malformed fields, strict duplicate/unknown-key decoding, finite bounds, enum consistency, exact raw source bytes/hashes and canonical golden output. Independent source inspection confirms receiving sensor/adapter IDs never become the transmitting radio; transmitter mapping requires known BSSID and an explicit assignment reference. The adapter cannot authenticate the caller's registry claims, which remains a caller responsibility.

## Architecture and scientific meaning

Foreign DTOs remain outward and private. The canonical envelope stores actual capture time, position, dwell and result age as Unknown; source result/API clocks use the separately versioned response wrapper. No API duration becomes channel dwell or RF capture time. An ambiguous CoreWLAN channel number is retained in exact source bytes and does not become a canonical primary channel/frequency. No unsupported noise, PHY, monitor, spectrum, position or radio identity is synthesized. Synthetic origin remains explicit even for empty terminal streams. Raw metadata can retain unredacted identifiers only under the explicit matching policy; raw network packet payload is absent.

Decoding requires the complete bounded byte slice before publication; there is no blocking transport I/O here. Limits and error messages are explicit, and normalization fails atomically for invalid mapping/privacy/canonical conversion. The maximum 68 MB stream plus retained/normalized copies requires worker scheduling and acquisition limits; mid-call cancellation and worst-case allocation are not validated by this unit. The author's release benchmark (4096 observations, about 10.3 MB) is approximately 89 ms decode and 5.8 ms normalization, not a UI-thread latency guarantee.

## Immutable reviewed files

| File | SHA-256 |
|---|---|
| `crates/capture-adapter/Cargo.toml` | `9d794ac9c3b7570d542b8bbf99b5331898ca15e9c7561ccf8fe06779e6749c69` |
| `crates/capture-adapter/src/json.rs` | `4b3935ec42e5757bc39cb64322f0f77f7146c2270372a772547ab46481815e59` |
| `crates/capture-adapter/src/lib.rs` | `d4c65cd470c385ab0f4593772bc9658a62c67202c1fe77ac1754c466d93a0eb4` |
| `crates/capture-adapter/src/macos.rs` | `97fa8de46d8ee393f99e2cf9ef51843c2173bea01149d24a54d89eca174cb827` |
| `crates/capture-adapter/src/macos/normalize.rs` | `e29665aa3d48d2f117159acc9fa17041a2fad90e9a276d1208bcc2e1b15b8aee` |
| `crates/capture-adapter/src/wire.rs` | `1b6233d57de59c00c203e9836296d04c380b46e7f5445f145e23a636c1c7ffd4` |
| `crates/capture-adapter/tests/fixtures/macos-valid-canonical.json` | `f265949aa82716d5119cfd128587431213b1f18bf53fad9ac377e28e4e163807` |
| `crates/capture-adapter/tests/macos.rs` | `b7c6023625711a2d4127e6606427012902196eb4a9d70e605a540117a48329fa` |
| `crates/capture-adapter/tests/normalization.rs` | `bcccb4227502d1d2ecc7384f8809eeac6bf2e6121db5321ee55da4e183d9e544` |
| `docs/adapters/capture-normalization.md` | `323121d01431e231889872fa6bd37b63f1c1b9e29a5cc60016aed2ab82558436` |
