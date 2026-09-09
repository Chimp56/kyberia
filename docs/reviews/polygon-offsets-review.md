# Rounded polygon offset review

Original candidate: `3406d6b`. Correction: `21b156e8`.

Beauvoir’s initial independent review found a MAJOR completeness defect: the
global buffered result could lose part of a tiny component and still intersect
its independently buffered result. The regression uses a large rectangle from
(-1000,-1000) to (0,0), a tiny disjoint rectangle from (2e-6,2e-6) to
(2.1e-6,2.1e-6), and outward distance 1e-5. Partial intersection was not proof
of complete coverage.

Laplace corrected the check to require coverage and added that regression.
Independent reviewer Russell approved `21b156e8` with no BLOCKER or MAJOR
findings after mixed-scale, translated, overlapping, inward and hole probes.
Uncertain cases failed with `UnsupportedCoordinateResolution`; the reviewer
observed no accepted component/hole loss in those probes. This is finite
adversarial evidence, not a mathematical proof of the floating-point kernel.

The one NIT, GEO-OFFSET-001, is addressed in the validation guide: the fallback
tolerance is `max(1e-7, 32 * f64::EPSILON * extent)`, not a hard 1e-7 ceiling.

Reviewer commands passed: 38 geometry package tests, workspace tests, geometry
Clippy with warnings denied, formatting, architecture, source inventory
(241 packages), and diff checks. Original probes are retained in the candidate
worktree at `.trash/review-probes/offset-correction-21b156e8`.

The existing native/WASM square proof for `3406d6b` does not by itself validate
the corrected coverage fallback on every geometry or platform. Gate E import,
3D, precision and broader geometry requirements remain open.
