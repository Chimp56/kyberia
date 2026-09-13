# ADR-0033: Tauri/React desktop shell and versioned IPC

- Status: Proposed bounded shell boundary; product lifecycle gates remain open
- Date: 2026-09-13
- Plan alias: ADR-006
- Related: plan §§10.1–10.4, 10.13, 14, 16, 20; FND-001; ADR-0001, ADR-0002, ADR-0029

## Context

RF Atlas needs a desktop shell that can present project state and invoke the
canonical Rust application boundary without giving a renderer direct access to
filesystem or storage types. The shell also has to keep project admission,
native selection, progress, cancellation, error mapping, and session
publication coherent when commands overlap or workers stop unexpectedly.

The plan names this decision ADR-006. Repository ADR numbers are unique.
ADR-0006 already records the Sionna worker boundary, and the immediately
preceding repository numbers are reserved by parallel integration work, so
this record uses ADR-0033 and preserves the plan alias explicitly.

## Decision

Use a Tauri 2 shell with a React/TypeScript renderer and a Rust application
adapter. The renderer communicates through the versioned
`kyberia.desktop-ipc/1` DTOs in the Tauri command boundary. It submits
caller-owned UUID job identifiers, receives strict progress and cancellation
responses, and receives opaque single-use project grants instead of paths.

The Rust shell admits one project operation at a time, owns native picker child
processes, polls cancellation and a bounded timeout, terminates and reaps a
cancelled or timed-out process, and maps unsupported platform capability to an
explicit error. `project_select_open` is an async Tauri command: admission is
recorded before the owned picker is moved to `spawn_blocking`, so the command
handler remains available for concurrent status and cancellation requests
against the same shared state. Picker cancellation and timeout return
structured termination and wait evidence; a missing wait status is a retryable
error rather than a successful cancellation. A create worker publishes its
new session only after the worker has joined successfully; a bundle created
before a worker panic or cancellation is moved to the owning `.trash` recovery
bin. Existing sessions remain queryable when a replacement operation fails.

This decision records the bounded shell and application boundary. It does not
claim that floor-plan import, calibration, native capture, analysis workflows,
authorization, or the complete product lifecycle are implemented.

## Alternatives

- Give the renderer direct filesystem access: rejected because path handling,
  storage invariants, and error classification would be duplicated in UI code.
- Use Electron with a Node filesystem bridge: rejected for this increment
  because the repository already has a Tauri/Rust application boundary and
  the extra runtime would duplicate ownership and packaging concerns.
- Expose application or storage session types directly over IPC: rejected
  because the outer contract would couple the renderer to domain and storage
  representations.
- Leave native picker commands detached or use `Command::output()` directly:
  rejected because cancellation could not close the picker or reap its child,
  and a worker could outlive its admitted job.
- Use unversioned command payloads: rejected because renderer and shell
  changes would have no explicit compatibility boundary.

## Evidence

- `apps/desktop/src-tauri/src/lib.rs` owns DTOs, job admission, opaque grants,
  session publication, cancellation cleanup, and panic-safe create finalization.
- `apps/desktop/src-tauri/src/main.rs` owns the native picker process adapter,
  absolute platform command paths, the async progress-aware Tauri commands,
  shared-state status/cancellation boundary helpers, and child-process
  cancellation/timeout/reaping tests. The boundary regression keeps a real
  picker child live while it invokes status and cancellation through the
  shared command-state path, and asserts both termination and wait evidence.
- `apps/desktop/src/lib/contracts.ts` and
  `apps/desktop/src/lib/useProjectSession.ts` validate the versioned IPC,
  retain renderer-known job identity, and expose honest cancellation states.
- `apps/desktop/tests/e2e/desktop-shell.spec.ts` exercises rendered loading,
  cancellation, session preservation, operation serialization, retry, and
  strict progress/cancellation acknowledgement behavior.
- `apps/desktop/src-tauri/Cargo.toml` and `Cargo.lock` pin the direct desktop
  Rust dependency versions; `apps/desktop/package-lock.json` pins the direct
  renderer and CLI graph.
- `docs/validation/desktop-shell.md` records the commands, evidence, and
  platform limitations for this bounded increment.

## Consequences and limits

The shell has one admission owner for create, open, current, and native
selection work. UI code can show progress and cancellation while a native
picker remains open, and a cancelled create has a recoverable exact bundle
instead of silently discarding storage. The opaque grant limits path exposure
and expires stale selections. The selected Tauri, React/TypeScript, and
workspace rows remain `IN_PROGRESS` until an independent review confirms the
bounded implementation; the ledger does not claim complete product lifecycle
delivery.

Native picker behavior remains host-dependent: supported hosts can terminate
the owned selector process, while unsupported targets return a capability
error. The current application projection has no calibration proof, and
import, capture, analysis, authorization, and durable product workflow
integration remain open gates.

## Reversibility

The renderer and Tauri command adapter can be replaced behind the same
versioned application-owned DTOs. A future native picker library can replace
the process adapter without changing grant or session contracts. Adding a new
IPC schema requires an explicit version and migration path; existing schema
behavior remains available until its callers migrate.

## Validation

Author-side validation for this increment includes the desktop Rust unit and
boundary tests, a real shared-state async-command concurrency regression,
real child-process output/cancellation/timeout tests, and a structured
termination-plus-wait assertion that fails if reaping is reduced to kill-only.
It also includes the renderer type/unit/build checks, Playwright lifecycle
tests, the no-bundle Tauri build, workspace checks, architecture and
source-inventory checks, and ledger consistency checks. These checks are
evidence for the bounded shell boundary only. Independent review and the
product lifecycle gates listed above remain open.
