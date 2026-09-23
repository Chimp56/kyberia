# Spectrum evidence contract

This crate is a hardware-independent, versioned boundary for imported or
future-acquired spectrum sweeps. `SpectrumSweep` validates a frequency grid,
acquisition settings, calibrated or explicitly unknown bin values, calibration
profile metadata, clock evidence, and pose evidence before producing canonical
JSON and a SHA-256 content identity.

`SpectrumEvent` binds a deterministic, deliberately narrow pattern assessment
to the exact ordered sweep content. Rule set version 2 can emit only
narrowband-persistent, wideband-persistent, or unknown pattern labels. The
milli-dBm threshold is used only with dBm-per-bin evidence; dBm/Hz sweeps are
preserved but assessed as unknown until equivalent-noise-bandwidth
normalization is modeled. Bin spacing alone is not used to convert PSD.

A bin can support a persistent pattern only when it is determinate in at least
80% of all sweeps in the event and meets the conditional persistence rule.
Global grid support is reported separately and cannot compensate for
frequency-local gaps. The support score is not a probability and does not
identify an emitter or protocol. Missing calibration, clipped evidence, gaps,
inadequate cadence, or insufficient observations remain unknown. Position and
time unknowns are retained rather than inferred.

The API does not acquire samples, apply calibration corrections, implement a
SoapySDR or vendor adapter, connect a remote sensor, or satisfy Phase 5 runtime
acceptance. Contract fixtures are synthetic and test serialization,
validation, provenance, deterministic identity, and conservative unknown
handling only.
