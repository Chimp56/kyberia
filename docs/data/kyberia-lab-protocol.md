# Kyberia Lab protocol v1

The coordinator signs canonical JSON recursively sorted by object key with Ed25519. A request binds schema version, run ID, pinned host ID, admitted 40-hex Git SHA, suite ID, seed, timeout, closed parameter map, 256-bit nonce, UTC issue time, coordinator key ID and command-spec version.

Before submission, the coordinator sends a fresh random challenge and verifies a signed host proof against the pinned host SPKI identity. Only then may it queue work. The runner returns strict JSON containing a payload and Ed25519 signature. The payload binds schema version, SHA-256 digest of the complete signed request, host ID, terminal status, UTC start/finish times, bounded stdout/stderr and reported capabilities. Capabilities must be a subset of the coordinator's pinned list.

The coordinator manifest binds all request/result identities, host SPKI identity, sorted capabilities, timestamps/status/spec version, the host result signature, and every sanitized artifact's name/media type/byte length/SHA-256/policy/truncation flag. The manifest has a separate coordinator Ed25519 signature. Verification must supply the expected run and host to prevent a valid old result from satisfying a new request.

No foreign lab object is persisted as Kyberia domain truth. Consumers may attach verified manifests as provenance evidence through a future explicit import adapter.
