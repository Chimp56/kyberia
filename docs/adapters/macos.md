# Native macOS CoreWLAN collector

The `collectors/macos` process uses Apple's CoreWLAN framework for real nearby-network scans and CoreLocation for authorization. It is an outward adapter; its JSON objects are not canonical domain objects. This unit implements capability probing, one-shot scanning, explicit authorization, and the validating process boundary. It does not finish the native collector catalog or Gate A.

## Build and run

Requires macOS with Xcode Command Line Tools, Python 3.9+, and the system Swift compiler. The tested toolchain is Swift 6.3.3, SDK macOS 26.6.2, arm64. No package download or third-party implementation is used.

```sh
python3 collectors/macos/build.py
collectors/macos/.build/KyberiaCollector.app/Contents/MacOS/kyberia-macos-collector probe
collectors/macos/.build/KyberiaCollector.app/Contents/MacOS/kyberia-macos-collector scan --timeout-seconds 20 --limit 256
```

The build compiles with warnings as errors, embeds the usage descriptions, builds an application bundle, ad-hoc signs it, and verifies the signature. Ad-hoc signing is for local development: stable distribution signing, notarization, and desktop application integration remain separate work. Build provenance hashes the Swift sources and Info.plist. A source change invalidates that hash; a runtime report additionally identifies the exact binary SHA-256.

`probe` and `scan` never request permission. The explicit operator command below requests CoreLocation consent only when global services are enabled and authorization is `not_determined`. It never requests GPS updates.

```sh
collectors/macos/.build/KyberiaCollector.app/Contents/MacOS/kyberia-macos-collector authorize --timeout-seconds 60
```

An interactive logged-in macOS session must respond to any system prompt. Denied/restricted states require the operator to change system privacy settings. A timeout is not consent or denial. Global Location Services disabled, authorization not determined, denied, restricted, and unknown remain distinguishable. A fresh permission check precedes each scan, follows its completion, and precedes publication of each observation. Revocation stops publication and preserves a partial terminal status if earlier observations exist.

The main thread runs a Foundation run loop, matching the run loop on which CoreLocation was initialized. A shared production-loop test verifies scheduled callback delivery; a dispatch-only loop would not establish this. Successful authorization reports the final authorization and service state, and the decoder rejects a successful transition from an initially nonrequestable denied/restricted/unknown state.

Options: `--interface en0` selects an interface; `--limit 1..4096` bounds observations; `--timeout-seconds 1..60` bounds the process lifetime. The default output redacts SSID and BSSID. `--include-identifiers` explicitly enables actual source-provided BSSID and base64 SSID octets for a local survey consumer; downstream storage must apply the project's privacy policy. Invalid arguments write only usage to stderr and exit 64.

## Version 1 NDJSON contract

Every record carries `protocol: kyberia.macos.collector/1`, a process session UUID, contiguous zero-based sequence, kind, receipt UTC, and monotonic receipt nanoseconds as an unsigned decimal string. `clock_epoch` identifies this process's monotonic timeline; cross-process clock alignment is not implied. No record exceeds 16 KiB including its newline. The defensive decoder in `contract.py` rejects duplicate keys, nonfinite values (including exponent overflow), malformed shapes, mismatched sources/sessions, missing terminal records, impossible scan sequencing, and invented known metrics.

| Kind | Meaning |
| --- | --- |
| `hello` | Collector version, source-build hash, OS version, privacy mode, limits, and evidence origin |
| `capabilities` | API-visible sources, interface power and reported bands, permission state, supported/conditional/unavailable fields |
| `authorization` | Explicit authorization command, initial permission state, global service state, whether a prompt was requested |
| `scan_started` | Source and scan UUID with API-call start time |
| `scan_observation` | A returned CoreWLAN network result and its full source/quality provenance |
| `complete` | Exactly one terminal status, reason, observation count, and partial flag |

The process supports at most 32 interfaces, 4096 observations and 4164 records; excess interfaces fail explicitly before scanning. API result collections are supplied by CoreWLAN, so their internal allocation cannot be bounded by this adapter; emitted output and lifetime are bounded. A stalled stdout consumer gets at most 500 ms per record before exit 74. An incomplete stream is never successful. Diagnostics stay on stderr and do not contain localized API errors or network identifiers.

| Terminal status | Exit | Interpretation |
| --- | --- | --- |
| `ok` | 0 | Requested operation completed; a probe is not an authorized scan |
| `partial` | 2 | Observation limit reached; retained evidence is incomplete |
| `unsupported` / `unavailable` | 69 | No accessible matching interface, unavailable power, or interface removal |
| `error` | 70 | CoreWLAN failure or resource limit, with structured reason |
| `permission_required` | 77 | Global services or authorization prevents scanning |
| `timeout` | 124 | Deadline expired, independent of the blocking scan API |
| `cancelled` | 130 / 143 | SIGINT / SIGTERM |

Cancellation terminates this process's work; the OS may finish an already issued scan internally. All statuses with retained observations except `ok` carry `partial: true`. A caller must inspect this terminal state. SIGKILL, startup failure, or a disconnected consumer may leave no terminal record, which the decoder rejects.

## Measurement meaning and unknowns

RSSI and noise are actual `CWNetwork` values with explicit dBm units and uncalibrated status. Only finite integer readings in the conservative accepted range -200 through -1 dBm are published as known; zero or outside-range readings are unavailable, never excellent signal. Width and band are taken only from recognized Apple enums, with raw enum values retained; an unsupported enum is unknown. The API-reported channel number is preserved without guessing center frequency or primary-channel geometry.

The API does not give a per-result capture timestamp, cache age, dwell time, calibration state, stable physical-radio identity, driver version, or firmware version here. The collector exposes these as unknown. Receipt times and API call windows are **not** over-the-air capture times or channel dwell. Wall-clock uncertainty is unknown. Interface identity is scoped to a collector process, and BSSID is never manufactured from SSID. Missing SSIDs/BSSIDs remain unknown. Identifiers are not claimed to be stable physical AP identities.

Monitor frames, channel hopping control, and raw payload retention are unsupported by this collector. Position is not collected. PHY/IE decoding is not implemented in this unit and is reported accordingly; 802.11 generation, MLO, puncturing, per-chain signal, and spectrum measurements are not inferred from RSSI or channels. This collector cannot satisfy the entire professional passive-survey feature set on its own.

## Validation and current native result

```sh
python3 -m unittest discover -s tests -p test_macos_contract.py -v
KYBERIA_MACOS_NATIVE_TESTS=1 python3 -m unittest discover -s tests -p test_macos_contract.py -v
python3 collectors/macos/runtime_check.py --output collectors/macos/evidence/runtime-default.json --execution-context default_workspace_sandbox
```

The opt-in native suite compiles the actual Swift permission decisions and process lifecycle. It tests all ten service/authorization combinations, actual deadline expiry, SIGINT/SIGTERM, and disconnected output. These are software lifecycle tests; the test process is explicitly synthetic and does not call or emulate CoreWLAN. The seven independent synthetic NDJSON fixtures cover valid, partial, empty, error, unsupported, denied, and probe states. Adversarial tests cover units, identity, privacy, source versions, ordering, bounds, and malformed payloads.

The reviewed correction adds Foundation callback delivery, impossible-consent transitions, source-specific powered-state rejection, and distinct SIGINT/SIGTERM exit validation. The macOS suite now contains 21 tests. These regressions test the actual lifecycle machinery without requesting location consent.

The committed redacted runtime reports retain only counts, statuses, build hashes and versions. On macOS 26.6.2, the workspace sandbox returned zero API-visible interfaces and disabled Location Services. The same signed binary outside that sandbox returned one interface, enabled services, and authorization `not_determined`. The scan correctly returned `permission_required` with zero observations. This proves the sandbox affects API access and does **not** establish missing radio hardware.

Authorized scans, actual RSSI/noise validity, permission-dialog interaction, revocation during an active scan, and long-duration hardware capture remain unvalidated. To validate them: run the signed bundle in an interactive session with access to CoreWLAN; run `authorize` and grant consent; rerun `runtime_check.py` with the appropriate execution-context label; compare known-network results against independent radio evidence without committing raw identifiers; revoke consent during a scan and verify termination. Denied results and synthetic fixtures cannot pass the authorized native runtime gate. Changing the report label does not itself change sandbox permissions.

## Primary API references and source provenance

Implementation was checked against the installed SDK's `CoreWLAN/CWNetwork.h`, `CWInterface.h`, `CWChannel.h`, `CoreWLANTypes.h`, and CoreLocation headers, together with Apple's [CWNetwork documentation](https://developer.apple.com/documentation/corewlan/cwnetwork), [CWInterface documentation](https://developer.apple.com/documentation/corewlan/cwinterface), and [CLLocationManager documentation](https://developer.apple.com/documentation/corelocation/cllocationmanager). Header/API signatures informed independent implementation; no Apple implementation, vendor dataset, GPL implementation, or external fixture was copied. Fixture licensing remains `NOASSERTION`, pending the repository's distribution review.
