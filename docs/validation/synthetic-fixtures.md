# Original synthetic research fixtures

These fixtures establish numerical and semantic acceptance inputs. They are not captured Wi-Fi measurements, calibrated material presets, a competitor execution report, or proof that a product engine implements these cases. Product implementations must consume these fixtures through their own contract tests.

Run from the repository root with Python 3.9 or later:

```sh
python3 tools/validation/fixtures.py check
python3 -m unittest discover -s tests -p test_research_harness.py -v
python3 tools/validation/fixtures.py hashes
```

Regenerate deliberately with `python3 tools/validation/fixtures.py generate`, inspect the diff, and request independent scientific review before changing golden values. The checker compares exact canonical JSON bytes; tests independently check formulas, permutations, and semantic invariants. Format version and generator version are separate. JSON values carry explicit units; no Python object becomes a Kyberia domain object.

`fixtures/synthetic-scenes/canonical-v1.json` contains 24 scenes:

| Cases | Exact intended claim | Limit |
|---|---|---|
| Open/one-wall/two-wall at 2.4, 5, 6 GHz | Far-field Friis power plus 0, 3, or 10 dB artificial insertion | No multipath, measured materials, or regulatory power claim |
| Frequency/material pairs | Two arbitrary insertion-loss curves differ by 4 dB at equal distance | No concrete/glass/vendor defaults |
| Slab/opening | Removing one 12 dB artificial obstruction restores 12 dB | Floor references are not extra obstruction planes |
| Reflection and knife edge | Geometric path/screen coordinates | Electromagnetic coefficients and radio-map values stay null until Sionna validation |
| Antenna rotation | World-to-local yaw and `3*cos(theta)` dBi lookup, including an off-axis receiver that distinguishes +45° from −45° yaw | Not a manufacturer pattern or normalized radiation-efficiency claim |
| Co/adjacent AP pair | Linear-power multiplication by explicit 1 or 0.01 coupling | No fabricated measured noise/SINR or universal spectral mask |
| Hidden-node-like | Transmitters below scenario sensing threshold can interfere at receiver | Collision probability remains unknown without a MAC model |
| Weak signal/high throughput, strong signal/congested, healthy LAN/slow WAN | Endpoint-attributed throughput is independent of RSSI | Synthetic diagnostic inputs cannot establish an actual root cause |
| Uplink limited | Unequal transmit powers give unequal link budgets | Policy threshold is illustrative |
| Evidence gap | TIN value inside hull, unknown outside | No automatic extrapolation or unsupported compliance pass |
| Roaming boundary | Explicit 5 dB hysteresis and interval-bounded transition | No invented authentication duration |
| Multi-floor candidate problem | Enumerated optimum of a precisely stated finite set cover | No claim of full RF/capacity/regulatory feasibility |

The free-space reference uses `P_rx_mW = P_tx_mW * (lambda / (4*pi*d))^2`, with `lambda = 299792458/f` meters. This is the far-field, matched-polarization, isotropic Friis case. The plan's additive loss and linear-power semantics are normative ([plan sections 7.10, 8.4, 16.5](../../plan.md)). Decimal baselines are stored explicitly and tested with a separate wavelength-domain calculation. The `1e-6 dB` tolerance covers rounding and ordinary f64 arithmetic only; it is not physical RF accuracy. Material curves and coefficients were chosen solely to catch mapping, sign, and unit errors.

`survey-v1.json` provides an original 25-point field with a discontinuity at `x=3`. Its declared truth is `-40 - 2*x - 3*y - (12 if x>=3 else 0)` dBm. Integer LCG32 state `(1664525*state + 1013904223) mod 2^32` supplies bounded jitter `(state mod 7 - 3)/2` dB. Seed 42 is pinned; no implementation-specific Gaussian RNG or wall clock enters bytes. Position covariance is artificial: the four-entry `synthetic_position_covariance_m2` is a 2×2 XY covariance in row-major order `[xx, xy, yx, yy]`, as declared by `position_covariance_layout`. Z is fixed at 1.5 m by the synthetic construction; this makes no measured vertical precision claim. Noise is unknown, and monotonic timestamps belong to a synthetic clock. East/west room labels make blocked holdouts executable. This field supports interpolation comparisons but does not itself establish uncertainty calibration or a winning interpolation method.

`fixtures/wifiheatmap-oracle/tin-v1.json` independently reproduces selected-BSS maximum, single-triangle barycentric interpolation, convex-hull unknowns, and a workflow checklist from the plan's audited behavior. The three points uniquely determine a Delaunay triangle. There is no general triangulation implementation here and no upstream GPL source, assets, or fixtures were inspected/copied to construct it. The audited revision is reference provenance only; `upstream_execution: NOT_RUN` prevents a false differential-runtime claim. The cancellation/save/iperf checklist is an acceptance specification, not a claim of implemented UI behavior.

Original source and redistribution disposition are recorded in [fixture-sources.json](../licenses/fixture-sources.json). Real captures, field holdouts, calibrated spectra, Sionna numerical outputs, and competitor screenshots must be added separately with their own provenance and permissions. They must not overwrite these synthetic classes. The [runtime gates](runtime-gates.md) keep those obligations open.
