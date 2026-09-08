# ADR 0015: Versioned Wi-Fi channel coupling boundary

- Status: Accepted bounded numerical contract; full PHY/MAC integration open
- Date: 2026-09-07

## Context

Plan §§7.11–7.12, 8.8, 8.11, and Appendix I require channel-aware coupling and
linear effective interference while preserving a strict boundary around
Kismet, Sionna, platform capture, and regulatory policy. A binary channel graph
or a sum of dBm values would lose spectral separation and produce invalid
physical results. A generic Sionna SINR also lacks Wi-Fi channel, activity,
receiver, and CSMA/CA semantics.

## Decision

Implement channel geometry and coupling in the pure `kyberia-wifi-semantics`
crate. `ChannelGeometry` is a closed versioned value with stable center
frequency mapping, nominal bonded widths, occupied 20 MHz segments, explicit
puncturing, conservative cfg80211-backed puncturing admission, and
canonicalized bonded centers. Regulatory availability and client legality are
intentionally separate.

The first coupling method is `Trapezoid20MhzReceiverV1`: fixed 1 MHz midpoint
bins, flat transmitter power over each active 20 MHz segment, and a desired
receiver response with a unit 20 MHz core and 5 MHz linear shoulders. For
interferer bin weights `tx_i`, the coefficient is
`sum(tx_i * receiver_response_i) / sum(tx_i)`, where `sum(tx_i)` is the
interferer's total integrated received power. It is bounded but asymmetric for
unequal widths; symmetry is only expected for corresponding equal-width
unpunctured geometries. It returns a bounded coefficient and never claims to
be a certified transmitter mask. Effective interference multiplies that
coefficient by received linear power and an enum-labeled utilization evidence
value.

Observation-set completeness is a required input. Complete measured or
complete scenario sets may produce a known zero for an empty set; incomplete
captures always produce an unknown aggregate. Unsupported geometry and
numerical failure are exposed through a typed aggregate status.

Canonical caller-supplied `RadioId` performs physical-radio deduplication.
Conflicting duplicate evidence and cross-radio BSS aliases fail closed. Same
BSS/MLD behavior is selected explicitly by `SameBssPolicy`, and every result
retains method/version, assumptions, input IDs, and per-input contributions or
exclusions. Unknown positive contributors make the aggregate unknown; explicit
zero remains a known linear zero.

## Alternatives

- Binary overlap/channel graphs were rejected because 2.4 GHz 5 MHz spacing,
  adjacent-channel leakage, receiver filtering, widths, and puncturing require
  a frequency-domain approximation.
- Summing dBm was rejected because powers must be added in the linear domain.
- Treating each BSSID/SSID as a separate interferer was rejected because a
  physical radio can advertise multiple virtual BSSs and MLD links need
  explicit policy.
- Reusing Sionna generic multi-transmitter SINR was rejected because its
  semantics do not include Wi-Fi activity, channel masks, receiver policy, or
  CSMA/CA.
- A regulatory database was deferred because jurisdiction and DFS/PSC rules
  are a separate evidence/policy contract and were not needed to establish
  stable frequency geometry.
- Arbitrary puncturing masks were rejected because legal EHT patterns are a
  constrained subset; the bounded V1 uses independently represented values
  from Linux cfg80211's validation table and reports unmodeled patterns as
  typed unsupported geometry.

## Evidence

The focused Rust tests provide independent mapping, puncturing, malformed-wire,
equal-width symmetry, unequal-width orientation, range, co-channel,
separation, 2.4 GHz overlap, linear-product, unknown, duplicate, same-BSS,
observation completeness, underflow, order, adversarial-power, bounded-input,
and property-based checks. The implementation and fixture assumptions are
documented in [the channel coupling architecture note](../wifi-channel-coupling.md)
and [validation record](../../validation/wifi-channel-coupling.md).

## Consequences

The first result is suitable for inspectable diagnostics and as a coefficient
input to later Wi-Fi PHY/MAC/capacity models. It is not final SINR, airtime,
CCA, or capacity. Every changed numerical rule needs a new method/version and
regression baselines. Unknown geometry, power, and utilization remain visible
and can prevent a false finite total.

## Reversibility

The public method is closed and versioned; a future standards-derived receiver
filter can coexist as another variant without changing stored V1 results. No
external process or foreign object model is coupled to the domain boundary.

## Validation plan

Run the focused tests and clippy, then workspace tests and architecture checks.
Add calibrated RF-lab mask/receiver measurements, jurisdiction policy tests,
and controlled AP/occupancy fixtures before upgrading this bounded contract to
final interference/SINR or planner truth.
