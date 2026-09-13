# ADR 0032: Authenticated allowlisted lab MCP boundary

Status: Proposed development-tooling boundary; independent review and physical-host gates open

## Context

Plan §§15–16 and Appendix I OPS-004 require authenticated remote execution, immutable inputs, bounded jobs, artifact hashes and reproducible CPU/GPU/platform evidence. Manual runs cannot preserve these guarantees consistently. A generic shell or SSH MCP would create an unacceptable remote execution surface.

## Decision

Provide a stdio-only MCP coordinator using the official TypeScript MCP SDK 1.30.0. MCP inputs are strict, bounded enums/identifiers. The coordinator admits only configured full Git SHAs and fixed runner executables. It signs a versioned request containing run/host/revision/suite/seed/timeout/parameters/fresh nonce/time/spec version and input-manifest identity. A host runner verifies that signature, freshness, one-time nonce, exact checkout, exact operation version and every unique tracked input file/hash, maps the request through its own static command allowlist, and signs the result. The seed crosses the command boundary only as a fixed `KYBERIA_LAB_SEED` value. The coordinator verifies exact request digest/host/capability bindings, sanitizes bounded text artifacts, hashes them and signs a deterministic manifest.

Only stdio is enabled. Remote transport is deferred until a separately reviewed OAuth 2.1 or mutually authenticated fixed-agent boundary exists. The MCP server offers no command, argv, environment, filesystem-path, URL or script input. Test fakes cannot be selected by production configuration.

Unix process groups provide bounded descendant termination with a hard post-cancel deadline. A retained fixed-catalog Windows helper uses the previously tested Kyberia Job Object primitive. The TypeScript runner fails closed on Windows pending native integration and hosted execution. Run intent is persisted before publication; terminal state and signed recovery evidence survive coordinator restart. Run directories and replay claims are retained under `.trash` and are never automatically removed.

## Alternatives

- Generic shell/SSH MCP: rejected because it makes authorization equivalent to remote code execution.
- Coordinator-held host private keys: rejected because self-generated challenges do not authenticate another host.
- Branches/tags or checkout-on-request: rejected because mutable references break evidence identity and introduce a Git mutation surface.
- HTTP without a complete authorization server: rejected; local stdio ownership is the current authentication boundary.
- Raw capture artifacts: rejected by default because capture evidence can contain client identifiers, location and payload metadata.

## Evidence

Contract tests use both the official in-memory transport and a real stdio client/coordinator/`ProcessExecutor`/runner chain with generated keys and a harmless fixed seed-reading command. They enumerate the exact tools/templates and exercise signatures, binding, tamper/replay/wrong-key behavior, authentication reservations, hard cancellation, restart recovery, failed publication, bounded sanitization, package inventory, traversal and artifact integrity. Runner tests reject changed, dirty, ignored, symlinked, duplicated and substituted inputs. The Windows fixed-catalog parser has injection and bound tests.

## Consequences

Lab evidence is repeatable and inspectable without becoming canonical product truth. Operators must manage Ed25519 keys and pinned host/check-out configuration. Adding a suite requires an administrator config change at both boundaries. Captures remain summaries unless a future ADR defines a safe artifact class. Windows Job Object, physical radio, Kismet, spectrum and CUDA execution still need their respective hosts.

## Reversibility and validation

The boundary is replaceable because signed request/result/manifests are versioned JSON and the MCP SDK stays outward. Revalidate dependency provenance, protocol fixtures, tamper/replay tests and real-host gates on any schema, SDK, runner or key lifecycle change.
