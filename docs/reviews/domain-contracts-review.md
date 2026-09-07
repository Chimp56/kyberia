# Independent canonical domain contract review

Reviewer: `/root/qa_spec_audit`. Author: `/root/domain`.
Final decision: **APPROVED for the selected foundational contract increment**.
The initial covariance MAJOR finding below was corrected and independently
retested. No unresolved BLOCKER or MAJOR findings remain in this review scope.

Scope: units, identity, explicit unknowns, time, position covariance, observation
envelope admission, runtime capabilities, Serde validation, dependency boundary,
and selected foundational tests. Relevant specification: plan §§7, 10, 11, 14,
23 and Appendix I. This is not a claim that the complete domain model exists.

The reviewed code was an uncommitted working tree at `/private/tmp/kyberia-domain`
based on `4e3bc5213e5a4b322ea56ef5253c24124f827f07`. Hashes identify actual reviewed
working-tree content; the preservation commit itself contains no domain code.

| Initially reviewed file | SHA-256 before correction |
|---|---|
| `crates/domain/src/capability.rs` | `62e8b3913e20fd58d099ce0a7404d540ac3dafd51ba2939fe2410a5e9fd76c3b` |
| `crates/domain/src/evidence.rs` | `223aab6a58164fee3f73a8ece31c0fd4aafab0cbd722d9f6b2079508695c470f` |
| `crates/domain/src/identity.rs` | `2438a332728671b14d49f5bfdb8dacd61ee1f16f2ff70300151bd7952d10be48` |
| `crates/domain/src/lib.rs` | `0df165ad1cdb423678bf54149c266127ce081126907838a081977fc27129dc9d` |
| `crates/domain/src/observation.rs` | `5927c54bbb1cd70919ad3c82d51b694a05646bb1cab75c0e4fe44038657863c9` |
| `crates/domain/src/spatial.rs` | `498b1f7e67c098a6d83edc6b205291ac2a9f0ecde1b01d5ba0c1a8c49c0ee6cc` |
| `crates/domain/src/time.rs` | `029083590ba7fd473bc981cb7210aff2b18b996aa93a4db77d3ae9013e8a2049` |
| `crates/domain/src/units.rs` | `0e5d37b96d0118badc8ec8c43c4772909e19ac8fbffba31663d10cd2fb04409b` |
| `crates/domain/tests/contracts.rs` | `5b115fe206f3b069df0696748ad5a746f18eac125ea8aee5f7d40a177cbfbd49` |
| `crates/domain/Cargo.toml` | `57128d3ce2850bdaeb0c5ba73dcc3f5e5158c58700052598b96813452a551341` |
| `docs/architecture/domain-contracts.md` | `d72f48d3636fa39070b889aead9c1874a5d3182cf8a2b709d03b45508dcf11d2` |

## Resolved MAJOR: invalid near-singular covariance admitted

`PositionCovariance::new([1.0, 0.0, 0.0, 0.0, 1e-8, 0.0])` succeeds although the
matrix has eigenvalues `1`, `1e-8`, and `-1e-8`. Its zero y/z variances cannot
have nonzero yz covariance. Global normalization followed by an absolute
`64 * f64::EPSILON` tolerance on principal minors accepts the `-1e-16` minor
and determinant, hiding a negative eigenvalue much larger than roundoff.

An independently compiled Rust probe linked against the built crate printed
`covariance_accepted=true`. The executable is retained in the system temporary
directory; no domain source was modified. Add constructor and deserialization
regressions for this matrix and scaled/permuted variants. Use a PSD criterion
whose numerical tolerance controls eigenvalue/pivot error, not merely the
degree-two/degree-three determinant products.

The author replaced the principal-minor test with diagonal-pivoted LDL
elimination. Residual entries/pivots now use the documented scaled tolerance;
negative original variances and nonzero covariance incident to exactly zero
variance are rejected directly. Constructor and Serde tests cover the original
counterexample, positive tiny-diagonal variants, and singular elimination.

Final independently reviewed changed artifacts:

| File | SHA-256 after correction |
|---|---|
| `crates/domain/src/spatial.rs` | `6773e951e185840a96355eac3300d2dc63848964bb6bd9370575e664ccea6401` |
| `crates/domain/tests/contracts.rs` | `dfa2199f5fa2450a25fe2717112f75183ddd2be27b76995c95b48a4bc84c2f11` |
| `docs/architecture/domain-contracts.md` | `4384e49787486b2eae2a1a5f22ad548d23339acfe24e793f931f2d5afd7c3738` |

After inspection, the reviewer reran the offline Cargo command below: all
18 contract/property tests and 7 compile-fail doctests passed. A separate
reviewer-compiled Rust probe exercised the counterexample and coordinate
permutations at scales `1e-200`, `1`, and `1e200`: all nine indefinite matrices
were rejected. Three correspondingly scaled singular PSD matrices were accepted.
The probes remain in temporary directories; no domain source was changed by
the reviewer and no recursive cleanup was performed.

## Passing observations and open boundaries

`cargo test --offline --manifest-path /private/tmp/kyberia-domain/crates/domain/Cargo.toml`
initially passed all 17 existing contract/property tests and 7 compile-fail
doctests. Those tests did not include the counterexample above. The corrected
18-test suite and independent numerical probe pass as recorded above.

Unit/ID Serde implementations route through validating constructors. Envelope
Serde routes through cross-field admission. Unknown noise survives distinctly;
cross-epoch durations/windows reject mismatches. The only production dependency
is Serde; no UI/storage/platform/Kismet/Sionna implementation leaks inward.

The domain documentation explicitly assigns bounded input framing/nesting to
outer adapters because Serde can allocate staging collections before admission.
It similarly assigns consent authentication and actual privacy transformation to
application/privacy services. These remain integration acceptance gates;
current pure value contracts do not claim to implement them. Channel regulatory
and PHY consistency is reserved for the Wi-Fi normalizer. No second
implementation-critical defect was identified in the selected increment.

No hardware, scientific RF accuracy, complete identity graph, active/spectrum
payload, coordinate-transform, migration, or UI capability is certified here.
