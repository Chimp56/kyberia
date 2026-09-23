# ADR 0034: bounded PNG map admission and operation-backed calibration

- Status: Accepted bounded increment; independent review passed after correction
- Date: 2026-09-23
- Requirements: plan §5.3, MAP-002, MAP-003, MAPB-001 and MAPB-002

## Decision

The first raster admission boundary accepts a strict PNG subset only. It
derives format and dimensions from bytes, bounds the source before parsing,
walks every chunk with checked arithmetic, verifies CRCs and ordering, admits
only IHDR, PLTE, tRNS, IDAT, IEND and a small fixed metadata whitelist, and
rejects trailing bytes. It does not inflate DEFLATE data. JPEG, TIFF, WebP,
PDF, SVG, CAD and geospatial formats remain unsupported until equally bounded
format-specific adapters exist.

For the admitted ancillary subset, ordering follows [PNG 3 §5.6,
Table 7](https://www.w3.org/TR/png-3/): `gAMA` and `sRGB` precede `PLTE` and
`IDAT`; `tRNS` follows `PLTE` if an optional palette is present and precedes
`IDAT`; `pHYs` precedes `IDAT`. CRC-correct regression fixtures exercise these
relationships, including rejection of metadata placed on the wrong side of
`PLTE` or `IDAT`.

PNG source bytes are content-addressed as immutable `MapSource` artifacts.
Application operations retain only the SHA-256 reference, canonical media
type, byte length, dimensions and caller-supplied opaque provenance ID; local
paths and filename metadata are not persisted. Filename extension and MIME
hints are non-authoritative.

Durable operation append, baseline registration and materialized publication
verify that each referenced map source exists, is registered as `MapSource`,
and matches its hash, media type and length. Hash verification streams
through a bounded buffer.

Operation schema V3 adds typed map import and calibration effects without
changing V1/V2 canonical identities. Typed priors prove absence of a newly
created map or calibration and preserve the prior active calibration state.
Undo removes the created entity and restores that exact prior. The causal
materializer checks floor-lock-sensitive map and calibration effects against
floor evidence binding before domain application. Floor identity follows
replay-visible map state across import and removal, including causally prior
V3 effects in the same operation set, so concurrent structural mutation and
evidence binding cannot depend on total-order metadata.

Application mutations preflight the candidate operation set against the
canonical baseline, append with an optimistic operation revision, rematerialize,
check the caller-owned cumulative budget and cancellation immediately before
publication, publish atomically, and return the canonical published revision.
Exact operation retries are idempotent.

## Failure boundary

Artifact registration and operation append are separate durable transactions.
A failure after artifact registration may retain an immutable, manifest-listed
orphan. It cannot become current project state without a separately validated
typed operation and canonical publication. Repeating the same bytes,
provenance and operation identity safely resumes the workflow. A semantic,
revision, cancellation or publication failure does not advance the canonical
current-project pointer.

## Consequences and open gates

This is a bounded MAP-002 foundation, not completion of its format catalog or
import wizard. PNG admission proves container integrity and bounded metadata;
it does not claim that the compressed pixel stream is decodable. Pixel
expansion must occur later in a separately resource-limited decoder/sandbox;
this boundary does not validate the IDAT zlib/DEFLATE stream or pixel contents,
and a renderer must successfully decode before treating the source as
displayable. MAP-003 is partial: operation-backed two-point transform
state and floor-evidence locks are represented, while multi-point controls,
residuals, CRS and versioned evidence migration remain open. Preview/UI and
desktop acceptance are outside this increment. Phase 0 exit is not claimed.

The bounded implementation is integrated on `main` at `2670207`. The broader
independent review and PNG-ordering correction re-review are recorded in
[`map-asset-admission-current-review.md`](../../reviews/map-asset-admission-current-review.md)
and [`map-asset-order-fix-rereview-20260923.md`](../../reviews/map-asset-order-fix-rereview-20260923.md).
Approval is limited to this increment and does not imply complete map-format
support, pixel decoding/displayability, desktop acceptance or Phase 0 exit.
