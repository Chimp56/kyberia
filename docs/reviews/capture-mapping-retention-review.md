# Capture mapping retention integration review

Disposition: APPROVED. Author: root. Independent reviewer: Russell.
Reviewed sources: ff70362 and test-strengthening follow-up 4c47283.

NativeCaptureSession retains the exact MappingContext only after stream and
normalization admission succeed. It exposes an immutable reference, preserving
explicit canonical identity and privacy decisions even for empty captures.
Foreign process and clock UUIDs remain separate values. The domain does not
import the adapter context; this is an outward composition boundary.

Independent review confirms mapping keys and process identity are validated
before construction. The final tests use custom session, collector, clock,
source and observation IDs, resolving the prior minor test weakness. Empty
capture coverage verifies canonical IDs and receiver mappings without deriving
them from observations. No findings remain for this bounded increment.

Integrated validation: observation-pipeline library tests pass 35, zero failed,
one ignored (.tools/mapping-integrated-tests.log); all-target Clippy with
warnings denied, formatting, and traceability generation pass.

This is in-memory provenance retention, not durable capture-session storage,
a registry implementation, or product acquisition completion. The separately
reviewed spool recovery tests remain in the isolated spool branch until its
durable session identity requirement is satisfied.
