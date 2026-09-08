# Survey receipt-association review

Date: 2026-09-07

Reviewer: `/root/survey_assoc_review_luna`

Disposition: **APPROVED**

## Findings and resolutions

The first review rejected the increment because association receipt time could
advance strict active duration. The implementation now keeps separate strict
and general event watermarks, with a regression proving that receipt-only time
cannot satisfy `minimum_active_time` or permit `finish`.

The review also required exact V1 `Unknown(NotMeasured)` temporal uncertainty,
one observation-ID namespace across strict records and associations, early
association resource limits, correct test counts, and ADR indexing. A later
replay review found that serialized synthetic quality could bypass
`allow_synthetic`; configuration-aware snapshot validation and positive and
negative mutated-wire tests close that path.

The final review reports no BLOCKER, MAJOR, MINOR, or NIT findings.

## Validation reviewed

- focused association suite: 8 passed;
- complete survey package: 31 passed, 1 ignored benchmark;
- complete locked/offline workspace: passed;
- workspace Clippy with `-D warnings`: passed;
- formatting, architecture, source inventory, and diff checks: passed; and
- release point-survey benchmark: passed.

## Boundary

The accepted contract creates a versioned normalized point association from
receipt or unambiguous API-window evidence without changing capture time,
channel dwell, cache age, pose, or strict survey readiness. Native transport,
project persistence, UI evidence inspection, and broader spatial assignment
remain open integration work.
