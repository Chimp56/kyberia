# Independent macOS collector review

Reviewer: `/root/qa_spec_audit`. Original author: `/root/harness`; correction
coauthor: `/root`. Decision: **APPROVED for the initial native process and
validating adapter contract**. No unresolved BLOCKER or MAJOR finding remains.

## 1. Scope completed

Read-only review of real Swift CoreWLAN capture, permission lifecycle,
measurement semantics, source provenance, bounded NDJSON, process failure,
synthetic fixtures and redacted runtime evidence. Relevant plan requirements
include native capture, Gate A, capability honesty, timestamps, privacy and
isolated process boundaries. Only this report was authored by the reviewer.

## 2. Files reviewed

The author's worktree HEAD was `914b0390e1fd0a99d668cb85f12778bcbe434fd2`.
These 23 hashes identify the actual working-tree content reviewed, including
subsequent corrections; uncommitted code is not attributed to that HEAD.

| File | SHA-256 |
|---|---|
| `collectors/macos/.gitignore` | `99921edb369e12a35a87d768ae481f8c944573626abd4b8f3a1fe78846f30ba7` |
| `collectors/macos/Info.plist` | `28145168e46cd09f0891a6e1d3a639400d8f42a0f302833d75abce8f9f7a974c` |
| `collectors/macos/Sources/Permission.swift` | `fa19ec36b0dc67457843fe31f59b75aa68b75d02ebc9c11da2c38b13523041f6` |
| `collectors/macos/Sources/Wire.swift` | `757fdc68f517cd4e2b62b09757f85f8e6435c8762f9520487d28081377a25b5f` |
| `collectors/macos/Sources/main.swift` | `3e1c57327aa3bbfabbb60c877b8263c9ab411543c19a271cd4d326d43be9976d` |
| `collectors/macos/Tests/PermissionTests.swift` | `5e50800f4c3be65428dd64c26a1d3ebfe700381d896dafc637845c5508ecedcd` |
| `collectors/macos/Tests/ProcessHarness.swift` | `69b8a5062ca1ee91675d74489e305ed134e9130d4f7864717e24041d021cfd06` |
| `collectors/macos/build.py` | `a40d1c2fcacd1ef7de7ae2a66f2660457eca0c25f64f4f8b50da4dcf9a8b4de5` |
| `collectors/macos/contract.py` | `528d05fd1c0017ea1bfb9fa8d962905117654ec815ba49b3f166b34e7e00b716` |
| `collectors/macos/evidence/runtime-default.json` | `75087f3e8f1d5d289e9eb3d5b1965e6c5f5710b3ec2eeca55a9aa7bfd62f407d` |
| `collectors/macos/evidence/runtime-outside-sandbox.json` | `83172006131d18ab42ddb292f9db4bb7a21d72defc047b452ef22713d73f3b5f` |
| `collectors/macos/fixtures/README.md` | `5e45605ca7f0e33cd0671d829bd890f46a8fa6b11424551b8afec1561133a089` |
| `collectors/macos/fixtures/denied.ndjson` | `c84561ff1f701df94b306728268716239c9db07f11859bf50fa619238f887e94` |
| `collectors/macos/fixtures/empty.ndjson` | `add8e73fcf95ff65f9c3a92508a3f75c8715c93ec19682c6e0a951a2925a9ba8` |
| `collectors/macos/fixtures/error.ndjson` | `fd9228928fa4ca37eb050bf8f839053b8d6542adc6fc82d46dd54341d3466db1` |
| `collectors/macos/fixtures/generate.py` | `58a3e226ae79022ad7817f2556ef52061f2516f23ebcd74744ba0820257414b7` |
| `collectors/macos/fixtures/partial.ndjson` | `c97028c554decdaa86b1acb3640261f1ca60f78a0d2fa89e741b92b5c961c97d` |
| `collectors/macos/fixtures/probe.ndjson` | `36dd021e66d32d8024d1e1ea869c1018d3224b6d704615ff79e461483bbb4b24` |
| `collectors/macos/fixtures/unsupported.ndjson` | `b965bef2a0713604ec5353f52297101da9f3c5695e39fb24e65e45fae0a8a453` |
| `collectors/macos/fixtures/valid.ndjson` | `00e9ab54d0968b84a619d05239d2cf5707705084c73314d01eeecdfab5de500c` |
| `collectors/macos/runtime_check.py` | `509684aefd5d806e4363e188e84f0262ef4d966ab617e78d9b32b51c9268f081` |
| `tests/test_macos_contract.py` | `4e0b50b071df86905316c88278bcbe1bff4c553a961a11e6c6ab16dca6855ee8` |
| `docs/adapters/macos.md` | `4466872b7705a02a12aabea963b8b11c9c4f47cd8552f1cc99978fc9d107bf68` |

## 3. Architectural assessment

The native API stays in an outward process. Its versioned JSON is adapter
support data, not canonical Kyberia domain data. BSSID/SSID default to explicit
redaction; no raw packet payload, location updates or guessed physical identity
is collected. Session-scoped interface IDs and source/binary hashes preserve
provenance. Native enum mappings were checked independently against the installed
Apple CoreWLANTypes.h, and CWNetwork.h documents RSSI/noise as dBm.

The conservative signal range rejects zero/error-like values. Unsupported widths,
PHY, noise, cache age, capture time, dwell, calibration and position remain
unknown where appropriate. API request/receipt windows are not over-the-air
capture timestamps. No ordinary scan is labeled spectrum analysis.

## 4. Findings and regression tests

**MAJOR MC-001 — Resolved.** Decoder accepted authorization initially denied,
with no prompt requested, followed by successful `location_authorized` completion.
The original independent mutated fixture reproduced this. Success now requires
an initially authorized state or a requestable prompt transition, plus explicit
final authorized state and enabled services. The original attack is rejected,
even when forged final fields claim authorization.

**MINOR MC-002 — Resolved.** Adding powered-on en1 to a fixture allowed a scan
on powered-off en0 because the decoder checked aggregate availability only.
The correction validates the selected source's power. The original two-source
attack now fails.

**MAJOR MC-003 — Resolved.** CLLocationManager was created on the main thread,
but `dispatchMain()` did not service its Foundation runloop. Installed SDK
CLLocationManager.h states callbacks use the initialization runloop. An
independent Swift Timer probe timed out under dispatchMain (watchdog exit 86)
and delivered under RunLoop.main.run (exit 0), without requesting consent.
The corrected production code and native harness share `runCollectorLoop`,
which keeps the main Foundation runloop alive. Its scheduled callback regression
now passes. This repairs the software lifecycle but does not claim a tested
interactive OS consent dialog.

**MINOR MC-004 — Resolved.** Runtime evidence validation accepted cancellation
exit 130 only, despite production SIGTERM exit 143. Extracted exit validation
now checks SIGINT/130 and SIGTERM/143 separately and rejects mismatches.

Four targeted regression tests were added by the correction author. The
reviewer independently reran the original false-consent and powered-off-source
probes and the SIGTERM mapping. Native tests compile production lifecycle and
permission code; their test process is explicitly synthetic and never pretends
to measure RF.

## 5. Independent execution

```text
KYBERIA_MACOS_NATIVE_TESTS=1 python3 -m unittest discover -s tests -p test_macos_contract.py -v
PASS: 21 tests (16 contract, 5 native lifecycle)

codesign --verify --strict collectors/macos/.build/KyberiaCollector.app
PASS

Original independent malformed-stream reproductions
PASS: impossible authorization and powered-off source are rejected

Independent runtime exit validation
PASS: SIGTERM cancellation accepts 143

Independent redacted default-context native probe
PASS: complete ok, exit 0, zero API-visible interfaces; no permission request

Runtime evidence consistency
PASS: both refreshed summaries match current Swift/Info source hash and binary SHA-256
```

The native tests cover actual timeout, SIGINT, SIGTERM, disconnected output,
Foundation callback delivery and all ten permission/service combinations.
Contract tests check duplicate keys, exponent overflow, malformed shapes,
identity/privacy, source versions, sequencing, record limits and honest unknowns.
The reviewer did not rebuild the signed application after evidence capture;
compilation with warnings-as-errors and bundle build were executed by the author,
while the reviewer independently compiled native lifecycle tests and verified
bundle signing. No recursive cleanup or consent request occurred during review.

## 6. Known limitations

The redacted reports distinguish sandbox access from hardware existence: default
workspace access reports zero interfaces and services disabled; the author's
outside-sandbox execution reports one interface, services enabled and permission
not determined. Both scans stop with permission_required and zero observations.
The reviewer independently checked the default probe, source hashes and exact
binary correspondence, but did not independently repeat outside-sandbox capture.

## 7. Requirements supported

This increment provides an actual native collector executable, capability probe,
operator-invoked permission request path, real one-shot scan implementation,
versioned defensive stream decoder, provenance and partial/error semantics.
Process output is bounded and lifetime is cancellable independently of blocking
CoreWLAN scans. Limits and missing terminal records remain explicit failures.

## 8. Requirements still open

Authorized scan metrics, actual consent-dialog interaction, active-scan permission
revocation and long-duration hardware behavior remain unvalidated. Complete
Gate A, canonical adapter-to-domain normalization, persistent survey integration,
professional passive metrics, monitor capture, spectrum and desktop UX remain
separate work. Ad-hoc development signing is not distribution notarization.

## 9. Risks and follow-up validation

Run the signed bundle in an interactive session, explicitly request authorization,
respond to the system dialog, and compare authorized scans against independent
known-radio evidence. Revoke permission during capture and verify partial
termination. Retain redacted summary evidence, never raw identifiers in Git.
These actions require the actual user's OS permission decision; elapsed time,
mock fixtures and denied scans cannot stand in for consent or measurement proof.

CoreWLAN owns its internal result allocation; the adapter bounds emitted records
and process lifetime but cannot constrain the framework's allocation. A killed
or disconnected process may have no terminal record and must be reported as
incomplete. Partial-limit status detects omitted results; it is not a complete
streaming dropped-event telemetry implementation. Consumers must preserve
synthetic origin during fixture replay and apply privacy before persistence.
Future authorized integration must not infer missing frequencies, dwell, noise,
PHY or identities from these API-shaped records.

## 10. Suggested commit

`feat(capture): add bounded native macOS CoreWLAN collector`

Review artifact: `docs(review): approve corrected macOS collector contracts`.
