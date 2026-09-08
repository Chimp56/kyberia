# Trash clean command: pending review

Disposition: REQUEST_CHANGES on the in-progress draft, not a final candidate.
Reviewer: /root. Inspected draft SHA-256: `cd22653f9f090b6edb171b62c3bdfcb63b744d27384e7768ab954d0dbcd817f7`.

## MAJOR: configurable allowlist admits protected and nested paths

The draft `_validate_known_outputs` rejects absolute paths and `..`, but
accepts `.git`, `.tools` and `nested/target`. Root executed only this pure
validation function and confirmed all three were admitted; no files were
moved. A public `known_outputs` override therefore defeats the documented
complete allowlist and can traverse nested symlink ancestors. Remove the
arbitrary override or enforce the exact root-level supported output names.
Tests should use those same names inside isolated retained fixture roots.

## Required follow-up

Review the existence-check/rename and manifest replacement sequence against
the no-overwrite claim. Preserve explicit concurrency limitations rather than
claiming a preflight check is atomic. Avoid following a replaced output symlink
when determining its kind. The author has received these findings. Do not run
this command against actual build outputs until the corrected candidate passes
independent review. The implementation remains isolated.
