# Independent review: numerical spatial baseline

Date: 2026-09-07. Reviewer: `rf_numerical_review`, independent of the implementation author. Decision: **APPROVED for the bounded pure backend increment only**. No BLOCKER, MAJOR, MINOR or NIT correctness findings remain in the reviewed scope. This is not approval of a complete measured heatmap feature, Phase 0, Phase 1 or iteration 4.

## Scope and specification

Read all 6,201 lines of `plan.md`, including the audit and definition of done. Inspected canonical units, evidence/unknown semantics and the original TIN fixture before judging the implementation. Relevant requirements are §3, §7.9, §7.20–7.25, §11.7, §12.1–12.2/12.7, §14.2–14.5, §16.4/16.14–16.16, backlog ANA-001/002/004, iteration 4 and Appendix I SPA-002/005/007/010.

The source worktree was based on `2d69333a7ef664843cb1e526292d76d395842050`; the six new implementation files were frozen and uncommitted. Review did not edit these files. Canonical `crates/domain/src/units.rs` SHA-256 was `ec884a619b3569d5254285c8a0d38a2a3615add8d26d4a9e76370cfb6dd1a19e`. All quantities reject nonfinite inputs and canonicalize signed zero, which prevents a suspected total-order/coincident-group ambiguity.

## Exact reviewed artifacts

| File | SHA-256 |
|---|---|
| `crates/spatial-analysis/Cargo.toml` | `5e51dd3a45f61b29ecbe990af7cb5400500b72c690bd6184db5174d46af9e260` |
| `crates/spatial-analysis/src/lib.rs` | `3c7273fffd6a48fbee5948a68175c9102df61980fa14205e8476c5a7edd5442f` |
| `crates/spatial-analysis/src/model.rs` | `35d383d138d526300384c1514edc037584c350bba743fcc380d6fbbf8e88ecfd` |
| `crates/spatial-analysis/src/tile.rs` | `34b3f0c265787ba42dafbd87c673da033500b4c67e01c179960b6fa6fd2b0646` |
| `crates/spatial-analysis/tests/baselines.rs` | `863abfd2afa759ac5677987268f2138c5b9b8668a8c612dd18e44d2f225e9c2f` |
| `docs/architecture/spatial-analysis.md` | `c8e2d8da25bc6ddd1ea35d0c5bb4f6d2d33fd5ac404234198c3d0d57ba4c5b39` |

## Findings and limits

No change is required before integrating this increment. The implementation uses a bounded max-heap, deterministic location ordering and rescaled positive IDW weights. Coincident measurements are averaged in dBm with their observation identities retained. Counts distinguish unique locations from repeated measurements. Missing scalar inputs cannot contribute support, unknowns remain numerically unknown, and extrapolation is explicit and bounded. Exact-coordinate values remain labeled within the input evidence plane; synthetic input is not promoted to measured evidence. No external project objects or side effects enter the numerical crate.

The radius rule is an explicit model assumption, not a convex-hull test, barrier model, confidence bound or proof that an unsampled room is supported. A single location can extend a constant value within its configured disk, and a large radius can span an evidence gap. These semantics are clearly documented. The symmetric affine fixture is not misrepresented as general affine reproduction; the radial holdout deliberately proves a 10 dB center error. Unknown uncertainty is appropriate here; neither neighbor spread nor density is presented as a calibrated interval.

Performance requires application orchestration. The maximum permitted dense job took **5,040.383 ms** on this host. The synchronous core must run in a cancellable worker, and viewport jobs need smaller budgets. The API checks cancellation at most every 64 location-distance evaluations and between cells, but bounded synchronous construction, grid validation, result/input cloning and serialization are outside a hard deadline guarantee. The existing documentation describes these boundaries. This approval does not authorize calling a maximum-sized tile on the UI thread or claiming large-project rendering is validated.

Caller-supplied metric/source-artifact claims are not authenticated by this pure crate. Application integration must bind the exact immutable sample artifact, selected metric/transmitter/time/sensor scope and coordinate graph. Serialize-only output avoids an unchecked importer; a future importer must validate sizes, schemas, contributor references and class/value invariants. These surrounding requirements remain open rather than being silently considered complete.

## Independent validation

Host: macOS arm64, Rust 1.98.1. All commands below exited 0 after correcting a reviewer-only probe that initially inspected the nonexistent Evidence `reason` field instead of canonical `detail`. That initial failed assertion was a test-harness error, not a production defect.

From the frozen source worktree:

```sh
rustfmt --edition 2024 --check crates/spatial-analysis/src/*.rs crates/spatial-analysis/tests/*.rs
cargo test -p kyberia-domain -p kyberia-spatial-analysis --offline
cargo clippy -p kyberia-spatial-analysis --all-targets --offline -- -D warnings
cargo test -p kyberia-spatial-analysis --release --offline benchmark_tiles -- --ignored --nocapture
```

Results: 16 spatial tests, 32 domain tests and 7 domain compile-fail documentation tests passed. The explicit authored benchmark passed separately. Formatting and Clippy passed. Cargo regenerated only local workspace package entries in the source worktree lockfile during testing; that generated diff was inspected and the lockfile restored to its original HEAD bytes. Integration must update its own workspace lockfile and dependency policy.

Independent probes exercised:

- 100 coincident readings count once spatially and cannot satisfy a three-location gate with only two locations;
- signed zero and exact 3–4–5 diagonal radius inclusion, immediately outside-radius exclusion;
- IDW powers 0.01, 0.5, 1, 2, 8 and 64 against a separately calculated three-point inverse-power reference, with normalized-weight sum checks;
- coordinate rescaling from 1e-300 to 1e300 preserving a rational two-point result within 1e-12 dB;
- Euclidean nearest selection against a slightly shorter diagonal alternative;
- unsupported scalar evidence surviving numerical export without becoming support or a known cell;
- the full admitted 100-million-distance budget with 64 nonzero contributors in every output cell.

The independent standalone harness is reproduced below. It is retained under ignored `.tools/spatial-review-probe` in the review worktree; no production source was changed. Commands:

```sh
cargo test --manifest-path .tools/spatial-review-probe/Cargo.toml --offline
cargo test --manifest-path .tools/spatial-review-probe/Cargo.toml --offline --release -- --include-ignored --nocapture
```

Six adversarial tests passed in debug and release; the additional explicit performance test passed in release. Its values are synthetic and independently constructed, with no copied third-party data or field accuracy claim.

| Run | Cells | Locations | Neighbor limit | Build ms | Tile ms | Known / unknown |
|---|---:|---:|---:|---:|---:|---|
| Authored sparse | 10,000 | 100 | 8 | excluded | 3.843 | 7,220 / 2,780 |
| Authored sparse | 100,000 | 100 | 8 | excluded | 32.026 | 7,220 / 92,780 |
| Authored dense | 100,000 | 100 | 8 | excluded | 56.509 | 100,000 / 0 |
| Authored large input | 100 | 10,000 | 8 | 1.050 | 3.424 | not recorded |
| Authored larger input | 100 | 100,000 | 8 | 12.564 | 32.842 | not recorded |
| Independent maximum budget | 1,000 | 100,000 | 64 | 11.547 | 5,040.383 | 1,000 / 0 |

These are local single-run measurements, not stable CI thresholds or cross-platform evidence. Disk, rendering and serialization are excluded. Maximum-budget inputs are a 1,000-column unit grid, radius 1e6 m and IDW power 2; the radius is deliberately artificial to exercise every heap candidate.

## Ten-field handoff

1. **Scope completed:** independent numerical, evidence-class, unit, determinism, resource/cancellation and performance review of nearest/IDW numerical tiles.
2. **Files changed:** only `docs/reviews/spatial-baseline-review.md`; ignored standalone probes retained locally.
3. **Architecture decisions:** no new ADR or implementation decision; confirmed inward pure backend boundary and explicit caller provenance responsibilities.
4. **Tests added:** six independent adversarial tests and one explicit maximum-budget benchmark, reproduced below and retained as review support.
5. **Tests executed/results:** formatting, Clippy, 16 spatial + 32 domain + 7 documentation tests, authored benchmark, six independent debug probes and seven release probes all passed.
6. **Known limitations:** local CPU only, no field validation, statistical confidence, GUI, authenticated import or UI job scheduling validation.
7. **Requirements advanced:** backend portions of ANA-001/002/004 and iteration 4, with numerical review evidence; no complete product capability promoted.
8. **Requirements still open:** evidence drawer, tile cache/analysis hashes, spatial indexes, barrier/TIN/RBF/kriging/GP, blocked CV, uncertainty propagation/calibration, GPU/cross-platform parity, export/report integration and usable heatmap workflows.
9. **Risks/follow-up:** a maximum dense tile takes seconds; integrate worker/cancellation and viewport budgets before interactive use. Bind input artifact claims and scope before persistence. Do not map unknown uncertainty or unsupported cells to compliance passes.
10. **Suggested commit:** `docs(review): validate bounded spatial interpolation baselines`.

## Reproducible independent probe

Standalone Cargo manifest (paths are relative to the retained review harness):

```toml
[package]
name = "spatial-review-probe"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
kyberia-domain = { path = "../../../spatial/crates/domain" }
kyberia-spatial-analysis = { path = "../../../spatial/crates/spatial-analysis" }
serde_json = "=1.0.149"
```

`src/lib.rs` (fixture constructors use the reviewed public types; test assertions are independently specified):

```rust
#![cfg(test)]
use kyberia_domain::{
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{ContentHash, FloorId, FrameId, ObservationId, Text},
    units::{CoordinateMeters, Dbm, Meters},
};
use kyberia_spatial_analysis::*;

fn point(x: f64, y: f64) -> Point2 {
    Point2 {
        x: CoordinateMeters::new(x).unwrap(),
        y: CoordinateMeters::new(y).unwrap(),
    }
}
fn sample(id: u128, x: f64, y: f64, value: f64) -> Sample {
    Sample {
        observation_id: ObservationId::from_bytes(id.to_be_bytes()).unwrap(),
        floor_id: FloorId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([2; 16]).unwrap(),
        position: point(x, y),
        value: Evidence::Known(Dbm::new(value).unwrap()),
        position_covariance: Evidence::Unknown(UnknownReason::NotMeasured),
    }
}
fn config() -> Config {
    Config {
        method: Method::Idw { power: 2.0 },
        support_radius: Meters::new(5.0).unwrap(),
        minimum_locations: 1,
        maximum_neighbors: 8,
        extrapolation: Extrapolation::Disabled,
    }
}
fn inputs(samples: Vec<Sample>) -> Inputs {
    Inputs {
        floor_id: FloorId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([2; 16]).unwrap(),
        metric_definition: Text::new("test/synthetic-single-transmitter-rssi/1").unwrap(),
        evidence_plane: InputEvidencePlane::Synthetic,
        source_artifact: ArtifactReference {
            sha256: ContentHash::from_sha256([1; 32]),
            media_type: Text::new("application/kyberia-test-fixture").unwrap(),
            byte_length: 0,
        },
        samples,
    }
}
fn model(samples: Vec<Sample>) -> Model {
    Model::new(inputs(samples), config()).unwrap()
}
fn estimate(model: &Model, x: f64, y: f64) -> Cell {
    model.estimate(point(x, y), &mut || false).unwrap()
}
fn value(cell: &Cell) -> f64 {
    cell.value.as_known().unwrap().get()
}
fn grid(width: u32, height: u32) -> Grid {
    Grid {
        floor_id: FloorId::from_bytes([1; 16]).unwrap(),
        frame_id: FrameId::from_bytes([2; 16]).unwrap(),
        origin: point(0.0, 0.0),
        resolution: Meters::new(1.0).unwrap(),
        column_offset: 0,
        row_offset: 0,
        width,
        height,
    }
}

#[test]
fn independent_duplicate_weight_and_support_gate() {
    let mut samples = (1..101)
        .map(|i| sample(i, 0., 0., -40.))
        .collect::<Vec<_>>();
    samples.push(sample(101, 4., 0., -80.));
    let m = model(samples);
    let c = estimate(&m, 2., 0.);
    assert_eq!(value(&c), -60.);
    assert_eq!((c.support_locations, c.support_observations), (2, 101));
    let mut cfg = config();
    cfg.minimum_locations = 3;
    let m = Model::new(m.inputs().clone(), cfg).unwrap();
    assert_eq!(estimate(&m, 2., 0.).class, CellClass::Unknown);
}
#[test]
fn independent_signed_zero_and_diagonal_support() {
    let m = model(vec![
        sample(1, -0., 0., -40.),
        sample(2, 0., 0., -80.),
        sample(3, 0., 1., -60.),
    ]);
    assert_eq!(m.groups().len(), 2);
    assert_eq!(value(&estimate(&m, 0., 0.)), -60.);
    assert_eq!(estimate(&m, 3., -4.).class, CellClass::Interpolated);
    assert_eq!(estimate(&m, 3., -4.000001).class, CellClass::Unknown);
}
#[test]
fn independent_power_sweep_rational_reference() {
    for power in [0.01, 0.5, 1., 2., 8., 64.] {
        let mut cfg = config();
        cfg.method = Method::Idw { power };
        cfg.support_radius = Meters::new(100.).unwrap();
        let m = Model::new(
            inputs(vec![
                sample(1, 0., 0., -40.),
                sample(2, 4., 0., -80.),
                sample(3, 6., 0., -60.),
            ]),
            cfg,
        )
        .unwrap();
        let weights = [1.0_f64, 3., 5.].map(|d| 1. / d.powf(power));
        let expected = weights
            .iter()
            .zip([-40., -80., -60.])
            .map(|(w, v)| w * v)
            .sum::<f64>()
            / weights.iter().sum::<f64>();
        let c = estimate(&m, 1., 0.);
        assert!((value(&c) - expected).abs() < 1e-12);
        assert!((c.contributors.iter().map(|c| c.weight.get()).sum::<f64>() - 1.).abs() < 1e-14);
    }
}
#[test]
fn independent_extreme_location_scaling() {
    for scale in [1e-300, 1e-200, 1e-100, 1., 1e100, 1e200, 1e300] {
        let mut cfg = config();
        cfg.support_radius = Meters::new(scale * 5.).unwrap();
        let m = Model::new(
            inputs(vec![
                sample(1, 0., 0., -40.),
                sample(2, 4. * scale, 0., -80.),
            ]),
            cfg,
        )
        .unwrap();
        assert!((value(&estimate(&m, scale, 0.)) + 44.).abs() < 1e-12);
    }
}
#[test]
fn independent_rotated_three_four_five_geometry() {
    let mut cfg = config();
    cfg.method = Method::Nearest;
    cfg.maximum_neighbors = 1;
    let m = Model::new(
        inputs(vec![sample(1, 3., 4., -40.), sample(2, 0., 4.99, -80.)]),
        cfg,
    )
    .unwrap();
    assert_eq!(value(&estimate(&m, 0., 0.)), -80.);
    assert_eq!(estimate(&m, 0., 0.).support_locations, 2);
}
#[test]
fn independent_unknown_only_tile_roundtrip() {
    let mut s = sample(1, 0.5, 0.5, -40.);
    s.value = Evidence::Unknown(UnknownReason::UnsupportedCapability);
    let m = model(vec![s]);
    let t = m.tile(grid(1, 1), || false).unwrap();
    assert_eq!(t.cells[0].class, CellClass::Unknown);
    let json = serde_json::to_value(t).unwrap();
    assert_eq!(
        json["inputs"]["samples"][0]["value"]["detail"],
        "unsupported_capability"
    );
}

#[test]
#[ignore = "explicit worst admitted distance budget benchmark"]
fn independent_dense_max_budget_benchmark() {
    let mut cfg = config();
    cfg.support_radius = Meters::new(1e6).unwrap();
    cfg.maximum_neighbors = 64;
    let samples = (1..=100000)
        .map(|i| {
            sample(
                i,
                ((i - 1) % 1000) as f64,
                ((i - 1) / 1000) as f64,
                -40. - (i % 60) as f64,
            )
        })
        .collect();
    let start = std::time::Instant::now();
    let m = Model::new(inputs(samples), cfg).unwrap();
    let build = start.elapsed().as_secs_f64();
    let start = std::time::Instant::now();
    let t = m.tile(grid(100, 10), || false).unwrap();
    assert_eq!(t.cells.len(), 1000);
    assert!(t.cells.iter().all(|c| c.contributors.len() == 64));
    println!(
        "independent dense maximum budget: samples=100000 cells=1000 evaluations=100000000 neighbors=64 build_ms={} tile_ms={}",
        build * 1000.,
        start.elapsed().as_secs_f64() * 1000.
    );
}
```
