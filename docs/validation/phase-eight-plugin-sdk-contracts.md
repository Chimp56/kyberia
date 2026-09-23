# Phase 8 plugin contract foundation — candidate validation

Status: implementation candidate on `feat/phase8-plugin-sdk-current`; not
integrated; the follow-up independent review is pending. This is a portable
declaration and validation contract only, not a running plugin system.

## Plan scope

- `plan.md` §10.13, “Plugin system”: plugin categories and capability-scoped
  WASM host API direction.
- `plan.md` §14.7, “API stability”: semantic-version and capability
  negotiation.
- `plan.md` Phase 8, “Ecosystem and adjacent radios”: stable plugin SDK and
  versioned, portable projects.
- Partial evidence for `backlog:UX-012:1` and `audit:EXT-001:1` only. This does
  not complete their broader acceptance requirements.
- No completion evidence for `audit:EXT-002:1` or `audit:SEC-002:1`.
- The untrusted-manifest path checks raw slice length before Serde, enforces a
  fixed 32-delimiter nesting ceiling, and then applies strict semantic
  validation. Callers still must bound reads before buffering; this is not
  general heap accounting or sandbox enforcement.
- This correction addresses the parser-boundary finding in
  `docs/reviews/phase8-plugin-sdk-current-review.md`; the independent
  follow-up review is pending, not passed.
- A fixed v1 canonical-manifest byte and project-reference digest vector pins
  this Rust serialization implementation; cross-language interoperability is
  not claimed.

## Focused checks

| Command | Result and scope |
| --- | --- |
| `cargo test -p kyberia-plugin-sdk --locked --offline` | PASS: 23 focused contract tests; no unit or doc-test failures |
| `cargo clippy -p kyberia-plugin-sdk --all-targets --locked --offline -- -D warnings` | PASS: SDK crate and test targets, warnings denied |
| `cargo fmt -p kyberia-plugin-sdk --check` | PASS: SDK crate formatted |
| `python3 tools/architecture.py` | PASS: reviewed dependency direction and external-package boundary |
| `python3 tools/source_inventory.py check` | PASS: 522 locked external packages |

The sorted `(name, version, source)` identity digest for the locked external
package set is unchanged across the inventory refresh:
`c4f386a190a6a4e8ad459c16fa57cf87bbaa8a863aaf8518693096688e99eb9c`.
The registry also validates host policy when building an empty registry, so
invalid host limits cannot be hidden by having no plugin declarations.

## Limits and open evidence

The independent follow-up review is pending. The WIT source has not been parsed
or compiled because neither `wasm-tools` nor `wit-bindgen` is available in this
environment. No WASM component is loaded or invoked. No signature/trust policy,
host grant enforcement, process isolation, filesystem/network mediation,
cancellation, atomic output publication, or runtime CPU/memory/time limit is
implemented or tested. Resource declarations are validated against
host-advertised ceilings but are not enforced. No third-party collector,
metric, or export integration is claimed. WIT tool validation, sample plugins,
runtime/security enforcement, and Phase 8 exit criteria remain open.
