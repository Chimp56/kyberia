# ADR 0035: bounded native PNG map workflow

- Status: Proposed bounded adaptation; independent review pending
- Date: 2026-09-23
- Requirements: plan §5.3, MAP-002, MAP-003, Phase 0 §17, MAPB-001, MAPB-002

## Context

Mainline has a strict bounded PNG container admission and durable map/calibration
operation model, but the desktop adapter cannot create a canonical floor or
import a map through its UI boundary. `CreateProject` intentionally creates an
empty project, application map calls expose caller-built causal metadata, and
IPC `/1` has no map projection. Replacing the parser or cherry-picking the old
workflow candidate would discard current-main storage, order and causal fixes.

## Decision

This isolated current-main adaptation creates one explicit revision-zero
site/building/floor hierarchy for a fresh desktop project. The floor is a
canonical spatial baseline, not a plan or a claim that one has been imported.
The application-facing import/calibration intents accept stable operation and
actor identity only; they derive parents, logical time, causal depth and
expected revision from canonical persisted state. Durable outcomes separate
the append receipt from a subsequent canonical readback, so a readback failure
cannot be presented as an uncommitted mutation.

The desktop picker opens a regular file without following Unix symlinks or
Windows reparse points, binds a bounded one-shot opaque grant to the active
project/floor, and stages at most 32 MiB in cancellable chunks. Grants and
retained retry bytes are count- and time-bounded; retries retain the original
project and immutable import intent. Selection responses contain only an
opaque grant, display name, byte length and kind. IPC `/2` exposes canonical
map/floor metadata and persisted calibration scale, never a filesystem path or
raster bytes/pixels.

Only numeric two-point scale controls are exposed. Both finite points must be
distinct and strictly inside the admitted PNG dimensions; the scale appears
calibrated only when the canonical project records known active calibration
evidence. Competing renderer project/map actions are single-flight, pollable
and cancellable; stale completions are ignored. Cancellation/terminal grant
failure drops the grant and requires a fresh native selection.

## Preserved boundaries

- Keep the current-main strict PNG parser, ancillary ordering and immutable
  artifact verification unchanged.
- Do not inflate or decode IDAT, display a preview, or claim valid/displayable
  pixels. PNG container admission alone is not pixel validation.
- JPEG/TIFF/WebP, PDF/SVG/CAD/geospatial formats, multi-point or CRS controls,
  scale residuals, floor editors and surveyed-evidence migration remain open.
- The authored cross-platform selection adapters and all current fixtures are
  synthetic/mocked; no native picker runtime, two-platform fixture or field
  accuracy has been demonstrated by this increment.
- This proposal does not close Phase 0/1 or MAP-002/MAP-003/MAPB-001/MAPB-002.

## Alternatives

- Cherry-pick the older workflow candidate: rejected because its application,
  IPC and parser assumptions diverge from current main.
- Keep the current unavailable UI stub: safe but does not supply a usable
  initial-project-to-map workflow.
- Decode and render raster pixels in this increment: rejected because no
  separately resource-limited decoder/displayability evidence is in scope.

## Review and validation

The source candidate and its test report are in
[`desktop-map-workflow-current-main.md`](../../validation/desktop-map-workflow-current-main.md).
This ADR is proposed until an author-independent review checks the adaptation,
especially readback/retry behavior, platform-specific no-follow opens, grant
consumption/cancellation races, IPC projections and renderer single-flight
semantics. No integration is authorized by this draft.
