# Kyberia Lab MCP threat model

Assets are host signing keys, coordinator signing keys, immutable validation checkouts, test evidence, capture privacy and lab availability. Trust boundaries exist at the MCP client/coordinator, coordinator/runner process, runner/command, and artifact store.

Threats and controls:

- Tool-input injection: strict unknown-field-rejecting schemas, closed identifiers/enums and no command/argv/path/URL/environment inputs.
- Mutable or forged source: lowercase 40-hex admission, operator revision list, exact operation version, content-addressed operator input manifest, tracked ordinary-file enforcement, dirty/symlink/duplicate rejection, coordinator request signature and host response binding.
- Host impersonation: remote host-side private key, pinned SPKI hash and Ed25519 response verification. Remote transports resolve the host key in their host service and do not forward it from the coordinator. Same-host development mode has documented key co-residence and is not a remote-authentication proof.
- Coordinator impersonation/replay: runner-pinned coordinator key/key ID, bounded clock skew, random 256-bit nonce and atomic exclusive nonce claims retained on disk.
- Escape and denial of service: fixed commands, no shell, reservations covering authentication through durable admission, bounded request/queue/concurrency/time/output/manifest/artifact sizes, bounded buffering, hard post-cancel deadline and process-group containment.
- Evidence substitution: collision-safe run IDs, exclusive artifact creation, regular-file checks, SHA-256 inventory and signed canonical manifest.
- Privacy loss: summary/log artifacts only, identifier/secret/path sanitizer, truncation disclosure, no raw capture class, no raw host paths in MCP errors.
- Network exposure: stdio only. Remote HTTP is unavailable until standards-compliant authorization and transport authentication are implemented and reviewed.

Residual risks are sanitizer incompleteness for novel secret formats, administrator allowlist mistakes, compromised host keys and host-local attackers. Use least-privilege service accounts, offline key rotation, read-only immutable checkouts, OS sandboxing and short retention appropriate to the evidence.
