# Plugin contract foundation

This increment defines a portable manifest and declaration validator for
collector, metric, and export plugins. The Rust crate at
`crates/plugin-sdk` is the implementation; `schemas/plugin-wit/worlds.wit`
defines the corresponding language-neutral WASM Component Model worlds.

## Versioning and project identity

The manifest has its own numeric schema version (`rfatlas.plugin-manifest`,
version 1). The host API, plugin release, capability, and data contract versions
are stable `(major, minor, patch)` tuples. Compatibility uses a half-open
`[minimum_inclusive, maximum_exclusive)` range. Prerelease and build metadata
are not accepted by this version-1 representation; changing those semantics
requires a new manifest/canonicalization version.

The manifest is strict JSON: unknown fields and capability identifiers fail
closed, and v1 has no extension map. Capability requirements are ordered by
the closed capability enum. Canonical JSON bytes use the struct field order,
canonical namespaced capability/data-contract IDs, compact `serde_json`
encoding, and that canonical list order. A project reference binds the exact
canonical manifest hash and the exact WASM component byte hash and length. The component digest covers the
component blob itself, not a ZIP or other archive format.

The v1 canonical byte representation and its project-reference digest are
pinned by a fixed golden vector in the SDK tests. This is a Rust implementation
regression vector, not evidence of cross-language interoperability.

Plugin IDs use lower-case reverse-DNS labels. The contract carries no host
filesystem path, OS-specific path separator, ambient environment variable,
arbitrary network origin, shell command, or native-process request.

## Role and capability matrix

| Role | Input contract | Output contract | Exact host capabilities |
| --- | --- | --- | --- |
| Collector | `rfatlas.capture-batch` | `rfatlas.observation-batch` | `rfatlas.capture_events.read`, `rfatlas.observations.emit` |
| Metric | `rfatlas.metric-input` | `rfatlas.derived-layer` | `rfatlas.derived_layers.emit`, `rfatlas.geometry.read`, `rfatlas.observations.read` |
| Export | `rfatlas.export-view` | `rfatlas.export-artifact` | `rfatlas.exports.create`, `rfatlas.project.read` |

The validator requires the exact role input/output pair and capability set.
This prevents a collector declaration from acquiring metric or export
authority, and prevents arbitrary new capability names from silently entering
the contract. The WIT worlds import only the host interfaces for their role;
their return values pass through the same versioned `contract-payload` envelope.
Payload bodies are UTF-8 JSON and remain subject to host schema validation.

## Admission behavior

For untrusted serialized input, callers should use
`parse_and_validate_manifest(&[u8], host)`. It checks the original byte-slice
length against the host limit before calling Serde, then rejects nesting deeper
than 32 JSON object/array delimiters (including the root object) before parse,
and finally applies the semantic validator. Serde still enforces syntax,
duplicate-field rejection, unknown-field rejection, and typed field limits.
The typed `validate_manifest(&PluginManifest, host)` entrypoint is for values
already parsed by the caller; its size validation cannot bound parsing that
already happened.

The caller has already buffered the slice passed to
`parse_and_validate_manifest`; transport and file readers must enforce their
own byte bound before buffering. The SDK preflight prevents an oversized slice
from reaching Serde but is not a general heap quota or runtime sandbox.
`verify_component` checks supplied component bytes against the declared length
and SHA-256. The registry rejects duplicate plugin IDs and resolves only an
exact project reference; its fingerprint is independent of discovery order.

Resource values are requested ceilings. Passing declaration validation does
not mean a host has applied them. Likewise, a negotiated capability set is a
validated declaration, not an authorization decision or an enforcement token.

## Runtime boundary

This is a contract foundation. It does not bind the WIT worlds to a component
runtime, load or invoke a guest, establish trust/signature policy, inspect an
archive, enforce a sandbox or capability grant, mediate filesystem/network
access, deliver secrets, cancel work, atomically publish outputs, or enforce
CPU/memory/time limits. No third-party plugin is claimed to execute or be
sandboxed. A future host must implement and independently validate those
runtime responsibilities before the Phase 8 SDK or SEC-002 can be called
complete.

The current environment has no `wasm-tools` or `wit-bindgen` executable, so the
WIT source is included as the ABI definition but has not been compiler-checked
in this increment. That validation is a required follow-up before publishing
the contract to third parties.
