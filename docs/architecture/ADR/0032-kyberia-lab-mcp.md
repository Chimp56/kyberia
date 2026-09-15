# ADR 0032: Authenticated allowlisted lab MCP boundary

Status: Proposed development-tooling boundary; independent review and physical-host gates open

## Context

Plan §§15–16 and Appendix I OPS-004 require authenticated remote execution, immutable inputs, bounded jobs, artifact hashes and reproducible CPU/platform evidence. Manual runs cannot preserve these guarantees consistently. A generic shell or SSH MCP would create an unacceptable remote execution surface.

## Decision

Provide a stdio-only MCP coordinator using the official TypeScript MCP SDK 1.30.0. MCP inputs are strict, bounded enums/identifiers. The coordinator admits only configured full Git SHAs and a closed runner invocation union: either one absolute digest-pinned native executable with no arguments, or one absolute digest-pinned runtime with exactly one absolute digest-pinned self-contained bundle. No configurable arguments, inline code, loaders, imports, module selectors, shell flags or relative entry points can cross this credential boundary. It signs a versioned request containing run/host/revision/suite/seed/timeout/parameters/fresh nonce/time/spec version and complete-tree manifest identity. A host runner verifies that signature, freshness, one-time nonce, that the object is a commit, exact operation version and a complete bounded manifest of every ordinary blob in the commit. It materializes the already verified blob bytes into a new exclusive retained snapshot and runs only from that snapshot, so checkout dirt, ignored files and later mutations cannot become inputs. Git and operation executable files are absolute and SHA-256 pinned, and commands receive an empty `PATH`. The seed crosses the command boundary only as a fixed `KYBERIA_LAB_SEED` value. Coordinator/host key names and actual SPKI identities must be paired and distinct. The host sanitizes bounded output before signing. The coordinator persists the complete safe signed request, safe host payload and signed preimages; verifies exact request/host/capability/timing bindings; maps every job identity, logical tool ID/version/digest and artifact hash field-for-field into its deterministic signed manifest; and publishes no host filesystem paths. Coordinator-generated terminal evidence has an explicit origin and no fabricated host signature.

Only stdio is enabled. Remote transport is deferred until a separately reviewed OAuth 2.1 or mutually authenticated fixed-agent boundary exists. The MCP server offers no command, argv, environment, filesystem-path, URL or script input. Test fakes cannot be selected by production configuration.

Unix process groups provide bounded descendant termination with a hard post-cancel deadline. A retained fixed-catalog Windows helper uses the previously tested Kyberia Job Object primitive. The TypeScript runner fails closed on Windows pending native integration and hosted execution. Run intent is persisted before publication; terminal state and signed recovery evidence survive coordinator restart. Run directories and replay claims are retained under `.trash` and are never automatically removed.

## Alternatives

- Generic shell/SSH MCP: rejected because it makes authorization equivalent to remote code execution.
- Coordinator-held host private keys: rejected because self-generated challenges do not authenticate another host.
- Branches/tags or checkout-on-request: rejected because mutable references break evidence identity and introduce a Git mutation surface.
- HTTP without a complete authorization server: rejected; local stdio ownership is the current authentication boundary.
- Raw capture artifacts: rejected by default because capture evidence can contain client identifiers, location and payload metadata.

## Evidence

Contract tests use both the official in-memory transport and a real stdio client/coordinator/`ProcessExecutor`/runner chain with generated keys and a harmless fixed seed-reading command. They enumerate the exact tools/templates and exercise signatures, safe payload persistence/reopen, field and artifact-hash mapping, executable/loader/module tampering, key-role separation, tree-object rejection, replay/wrong-key behavior, authentication reservations, timing envelopes, hard cancellation, restart recovery, failed publication, bounded sanitization, package inventory, traversal and artifact integrity. Runner tests reject incomplete, changed, symlinked, duplicated and substituted commit trees and prove dirty, untracked and ignored checkout inputs are absent from the immutable snapshot. The Windows fixed-catalog parser has injection and bound tests.

## Consequences

Lab evidence is repeatable and inspectable without becoming canonical product truth. Operators must manage Ed25519 keys, generate complete commit manifests and refresh executable digests after approved tool upgrades. Executable verification relies on the host OS preventing mutation between the immediate digest check and process image loading; production runner/service accounts must not grant write access to pinned executables. Adding a suite requires an administrator config change at both boundaries. Captures remain summaries unless a future ADR defines a safe artifact class. Windows Job Object, physical radio, Kismet and spectrum execution still need their respective hosts. ADR 0043 removes accelerator selection from the Sionna tool surface.

## Reversibility and validation

The boundary is replaceable because signed request/result/manifests are versioned JSON and the MCP SDK stays outward. Revalidate dependency provenance, protocol fixtures, tamper/replay tests and real-host gates on any schema, SDK, runner or key lifecycle change.
