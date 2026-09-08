# Wi-Fi channel geometry and coupling boundary

`kyberia-wifi-semantics` owns the deterministic channel and interference
semantics required by plan §§7.11–7.12, 8.8, and 8.11. The boundary consumes
canonical `RadioId`, `BssId`, `MldId`, `ObservationId`, and unit-safe power
values from `kyberia-domain`; it has no capture, storage, UI, Kismet, or Sionna
dependency.

## Geometry

`ChannelGeometry` is versioned as `kyberia-wifi-channel/1` and validates:

- stable center-frequency mappings for 2.4 GHz channels 1–14, the supported
  5 GHz 20 MHz primary lattice `{36,40,44,48,52,56,60,64,100,104,108,112,
  116,120,124,128,132,136,140,144,149,153,157,161,165,169,173,177}`,
  and the 6 GHz `1 + 4k` 20 MHz lattice through channel 233;
- band-specific width compatibility: 2.4 GHz permits 20/40 MHz only, 5 GHz
  permits 20/40/80/160 MHz and 80+80 but not 320 MHz, and 6 GHz permits the
  listed 20/40/80/160/320 MHz and 80+80 forms when all segments are valid;
- primary frequency membership in the requested 20/40/80/160/320 MHz or 80+80
  geometry;
- complete low-to-high 20 MHz segment enumeration;
- explicit puncturing bits, with out-of-width and primary-segment puncturing
  rejected;
- canonical bonded centers: shifted centers are rejected unless they are within
  the wire tolerance of a standard center, in which case they are normalized
  before storage and serialization.

For 80+80 the two 80 MHz centers must be supplied low-to-high, have no
overlapping or touching occupied edges, and contain no duplicate 20 MHz
segments. Channel 14 is a special 2.4 GHz 20 MHz center at 2484 MHz and is
not a valid 40 MHz primary. These checks describe frequency geometry only;
country/regulatory availability, DFS/PSC state, power limits, and client
support remain separate policy/evidence inputs.

Puncturing admission is intentionally conservative. Nonzero masks are rejected
for 20/40 MHz, and 80/160/320 MHz accept only the finite value tables used by
Linux cfg80211's `valid_puncturing_bitmap`; the primary segment must remain
active. 80+80 currently accepts no nonzero mask because the Linux table does
not define that width. A legal pattern outside this bounded subset returns the
typed `UnsupportedPuncturingPattern` error rather than being treated as an
arbitrary mask. The reference values are independently represented from the
[Linux `net/wireless/chan.c` validation table](https://github.com/torvalds/linux/blob/1a15bf9708ba3bf80410065e113aa17cd6a18dcf/net/wireless/chan.c#L112-L175)
at commit `1a15bf9708ba3bf80410065e113aa17cd6a18dcf` (raw file SHA-256
`a58880f0b3225a5790e1874e6a88675e4ac0eb8098df432d597ca1549d79ded0`) and do
not create a Linux runtime dependency.

The mapping deliberately does not claim regulatory availability. DFS, PSC,
power spectral density, country rules, client support, and current channel
state remain separate policy/evidence inputs. A malformed geometry is rejected
as invalid; an unavailable future numbering value is represented as unsupported
at the evidence boundary.

## Coupling approximation

`Trapezoid20MhzReceiverV1` is an inspectable approximation, not a standards
emission-mask certification. Every active 20 MHz segment contributes twenty
1 MHz midpoint bins (`center - 9.5` through `center + 9.5` MHz) with flat
normalized transmitter power. The desired receiver has response 1 through
the 20 MHz core and a linear response to 0 across a 5 MHz shoulder on each
side. For interferer bin weights `tx_i`,
`c_ij = sum(tx_i * receiver_response_i) / sum(tx_i)`, where the denominator
is the interferer's total integrated received power. The result is a bounded
coefficient `c_ij` in `[0,1]`; it is asymmetric for unequal widths by design.

The implementation has explicit invariants:

- identical geometries return exactly `1`;
- corresponding equal-width, unpunctured geometries have the expected
  orientation symmetry; symmetry is not assumed for unequal widths or
  different puncturing masks;
- non-overlapping masks return exactly `0`;
- 2.4 GHz 5 MHz channel spacing produces nonzero overlap when the masks
  overlap;
- punctured segments do not contribute to the mask;
- no Sionna generic SINR is used or exposed.

Changing the mask shape, shoulder, grid, or normalization requires a new
closed method/version and a new golden baseline set. The result is a channel
coupling factor only; Wi-Fi CCA, CSMA/CA, OBSS, PHY/PER, association, airtime,
and capacity remain later Kyberia-owned layers.

## Effective interference

`effective_interference` evaluates the explicitly documented product:

```text
I_effective = Σ(c_ij × received_power_mW × utilization)
```

Utilization is an enum that requires one of measured advertised BSS load,
measured CCA, observed-frame lower bound, spectrum occupancy, inferred, or
scenario assumption. A bare probability cannot enter the API. Received power
uses `Evidence<LinearPower>` so explicit zero is distinguishable from unknown;
zero has no finite dBm representation and is rendered as `Unknown(NotMeasured)`
in the dBm view.

Inputs are grouped by caller-supplied canonical `RadioId`. Identical evidence
rows for one radio collapse into one contribution and retain all input IDs and
identity aliases. Conflicting rows for one radio, or a BSS alias owned by
multiple radios, fail closed as ambiguous.
Same-BSS/MLD behavior must be selected with
`SameBssPolicy`: coordinated self-exclusion, independent scenario counting, or
reject ambiguous relationships.

The result records method/version, policy, observation-set completeness,
aggregate status, assumptions, sorted input IDs, and per-radio
contribution/exclusion records, including original geometry evidence.
Complete measured or scenario observations may
produce a known zero for an empty set. An incomplete capture always keeps the
aggregate unknown, even if the received rows are known zero. Unsupported
geometry and numerical failure are typed aggregate states, so callers need not
inspect contributions to discover that the total is unsupported. A positive
received power multiplied by positive coupling and utilization that underflows
to zero is numerical failure rather than known zero.
