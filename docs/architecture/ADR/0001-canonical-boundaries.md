# ADR-0001: Canonical contracts and isolated adapters

Status: Accepted as a specification constraint. Runtime integrations remain unvalidated.

## Context

Plan §§3, 7, 10, 14 and Appendix I require Kyberia to own canonical evidence, geometry, identity, metrics, policy and reports. External schemas and side effects cannot become domain truth.

## Decision

Use the recommended Rust pure domain inside a local-first modular monolith. Versioned ports face outward to storage, native collectors and processes. Raw, normalized and derived records remain distinct. Kismet is an external API/file evidence producer; Sionna RT is adopted only inside an optional process worker. Deconflict is a neutral interchange/reference target; wifiheatmap is a clean-room behavioral reference only. The domain has no UI, storage, operating-system or foreign-engine implementation dependency.

## Alternatives

Forking an upstream application, linking Kismet, loading Sionna in the desktop, or persisting external domain objects contradict the authoritative specification. Microservices would add deployment complexity before evidence requires it.

## Evidence

The complete plan and Appendix I audit prescribe these boundaries. The independent initial specification audit confirms that no implementation exists yet; this ADR does not claim runtime proof of any upstream integration.

## Consequences

Only adapters understand foreign and canonical schemas together. Physical quantities and unknown states are validated at deserialization boundaries. Observation provenance survives normalization. Capability failures remain visible, without fabricated measurements or silent engine substitutions.

## Reversibility

Adapters and execution engines can be replaced while retaining canonical evidence. Changing an upstream disposition requires a new evidence-backed ADR and a corresponding plan amendment.

## Validation

Architecture dependency checks, invalid-unit compile failures, adversarial decoding, deterministic replay, missing-capability tests, and worker absence/crash tests. Platform and measured-data gates are tracked independently.
