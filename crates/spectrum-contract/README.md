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
normalization is modeled for event classification. Bin spacing alone is not
used to convert PSD in that threshold/classification path.

`SpectrumSweep::integrate_band` provides a separate, bounded derived display
calculation for one selected half-open frequency band. It requires both band
edges to align to the validated sweep grid and includes complete bins only.
For dBm/bin values it converts each selected bin to mW before summation. For
dBm/Hz values it integrates each constant-within-bin density using the grid's
explicit bin width, also in linear power. The result is rounded to the nearest
milli-dBm (half-way cases away from zero) only after summation. This operation
does not change canonical sweep/event bytes or hashes, apply calibration terms,
estimate a noise floor, classify an emitter, or provide fractional-bin
interpolation.

Every selected bin must be observed and unclipped for an exact total. Below-
detection, clipped, and not-observed bins instead produce an unknown outcome
with ordered per-bin causes and observed/usable coverage; none are treated as
zero. The requested range, selected-bin count, and work are bounded by the
caller-supplied `ProcessingLimits`. This software-only calculation is not a
rendered spectrum view or Phase 5 product/hardware acceptance.

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
