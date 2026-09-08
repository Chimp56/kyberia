# Wi-Fi channel coupling validation

This focused validation covers the bounded `kyberia-wifi-semantics` increment
for plan §§7.11–7.12, capability catalog PAS-004/PAS-005 and audit items
PHY-001/PHY-002.

The independent fixture set exercises:

- 2.4 GHz channels 1, 5, 13 and special channel 14; the band-specific 5 GHz
  primary lattice and 6 GHz `1 + 4k` lattice;
- 20/40/80/160/320 MHz compatibility, center/primary/segment validation,
  80+80 canonical ordering, non-overlap/non-adjacency, duplicate-segment and
  puncturing validation rules, including the conservative Linux cfg80211
  puncturing subset and typed rejection of unmodeled masks;
- shifted-center rejection and canonical serialization for contiguous and both
  80+80 blocks;
- malformed and future version wire data;
- exact co-channel `1`, bounded `[0,1]`, non-overlap `0`, 2.4 GHz overlap, and
  independent unequal-width receiver-weighting oracles;
- explicit 1 MHz midpoint-bin/trapezoid-mask assumptions, puncturing effects,
  equal-width symmetry, and unequal-width orientation asymmetry;
- linear power/utilization products, exact zero versus unknown evidence,
  positive-factor subnormal underflow as numerical failure, and
  overflow/resource limits;
- same-BSS/MLD policies, canonical-radio deduplication, conflicting aliases,
  deterministic order, and unknown/unsupported geometry propagation;
- property-based equal-width symmetry/range checks over 2.4 GHz channel pairs,
  deterministic stable ordering, alias visibility, and observation-set
  completeness/typed unsupported aggregate status.

The puncturing fixtures are value-level reproductions of Linux cfg80211's
`valid_puncturing_bitmap` tables in
[`net/wireless/chan.c`](https://github.com/torvalds/linux/blob/1a15bf9708ba3bf80410065e113aa17cd6a18dcf/net/wireless/chan.c#L112-L175),
reviewed at immutable commit `1a15bf9708ba3bf80410065e113aa17cd6a18dcf`.
The raw file SHA-256 is
`a58880f0b3225a5790e1874e6a88675e4ac0eb8098df432d597ca1549d79ded0`.
The source is GPL-2.0-only; Kyberia does not bundle or execute Linux code.
Any future source update requires re-running the value comparison as a
licensing and semantic review gate.

Focused commands:

```text
cargo fmt --all
cargo test -p kyberia-wifi-semantics --locked --offline
cargo clippy -p kyberia-wifi-semantics --all-targets --locked --offline -- -D warnings
```

The suite deliberately does not call a jurisdiction database or a spectrum
analyzer. Runtime regulatory validation, calibrated receiver-mask validation,
and field measurements remain separate gates. The approximation must be
compared with controlled RF fixtures before use as a planning truth.
