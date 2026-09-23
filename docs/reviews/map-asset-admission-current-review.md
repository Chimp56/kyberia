# Map asset admission current candidate review

Date: 2026-09-23

Status: **PENDING independent review**

Target: `feat/map-asset-admission-current-main`, adapted from current `main`
baseline `05953134d24666e8483cbfdb7d9aacd0ce4e6e48`.

This candidate adds bounded PNG admission, content-addressed map source
closure, typed V3 map import/calibration operations and materializer/store
integration. Historical review artifacts apply only to their named commits
and are not approval of this current-main adaptation.

An independent reviewer must inspect the complete plan and exact candidate
diff, especially V1/V2 identity preservation, source closure at append,
baseline and publication, cancellation/publication order, floor-lock conflict
equivalence under both operation-ID orders, same-floor/other-floor behavior,
same-set map import resolution, undo/redo/retry semantics, and bounded read
accounting. Reviewer disposition and any findings belong here before the
feature evidence is considered reviewed.
