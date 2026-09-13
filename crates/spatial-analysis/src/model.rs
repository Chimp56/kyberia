use crate::*;
use kyberia_domain::{
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{FloorId, FrameId, ObservationId},
    units::{Db, Dbm, Meters, Probability},
};
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Method {
    PointValue,
    Nearest,
    Idw { power: f64 },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub enum Extrapolation {
    #[default]
    Disabled,
    WithinRadius(Meters),
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub method: Method,
    /// Model policy, not a measured confidence bound or convex hull.
    pub support_radius: Meters,
    pub minimum_locations: usize,
    pub maximum_neighbors: usize,
    pub extrapolation: Extrapolation,
}
impl Config {
    pub fn validate(self) -> Result<(), Error> {
        if self.support_radius.get() <= 0.0 {
            return Err(Error::InvalidConfiguration(
                "positive support radius required",
            ));
        }
        if self.minimum_locations == 0
            || self.maximum_neighbors < self.minimum_locations
            || self.maximum_neighbors > MAX_NEIGHBORS
        {
            return Err(Error::InvalidConfiguration("neighbor limits"));
        }
        if let Method::Idw { power } = self.method
            && (!power.is_finite() || power <= 0.0 || power > 64.0)
        {
            return Err(Error::InvalidConfiguration("IDW power must be in (0, 64]"));
        }
        if let Extrapolation::WithinRadius(radius) = self.extrapolation
            && radius < self.support_radius
        {
            return Err(Error::InvalidConfiguration(
                "extrapolation radius below support radius",
            ));
        }
        Ok(())
    }
}

/// A group is one exact coordinate, not one independent statistical sample.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LocationGroup {
    pub position: Point2,
    pub observation_ids: Vec<ObservationId>,
    /// The exact aggregate, including method/version and ordered IDs, is
    /// retained alongside the convenient spatial location index.
    pub signal_aggregate: SignalAggregate,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Inputs {
    pub floor_id: FloorId,
    pub frame_id: FrameId,
    pub evidence_plane: InputEvidencePlane,
    /// Pins filtering, transmitter selection, aggregation and assignment
    /// versions. Construct it with the binding's validating constructor.
    pub metric_definition: MetricDefinitionBinding,
    /// Caller must verify that this immutable artifact contains these samples.
    pub source_artifact: ArtifactReference,
    pub samples: Vec<Sample>,
}
/// Synthetic fixtures remain visibly synthetic even at exact sample coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputEvidencePlane {
    Measured,
    Synthetic,
}
#[derive(Clone, Debug)]
pub struct Model {
    pub(crate) inputs: Inputs,
    pub(crate) config: Config,
    pub(crate) groups: Vec<LocationGroup>,
    pub(crate) barriers: BarrierSet,
}
impl Model {
    pub fn new(inputs: Inputs, config: Config) -> Result<Self, Error> {
        Self::new_with_barriers(inputs, config, BarrierSet::empty())
    }

    /// Construct an IDW model whose neighbor metric includes the supplied
    /// finite planar barriers. The legacy constructor intentionally remains
    /// Euclidean so existing metric identities and tile bytes are unchanged.
    pub fn new_with_barriers(
        mut inputs: Inputs,
        config: Config,
        barriers: BarrierSet,
    ) -> Result<Self, Error> {
        config.validate()?;
        if let Some(spatial_method) = inputs.metric_definition.spatial_method()
            && !matches!(
                (spatial_method, config.method),
                (SpatialMethod::PointValue, Method::PointValue)
                    | (SpatialMethod::Nearest, Method::Nearest)
                    | (SpatialMethod::InverseDistanceWeighted, Method::Idw { .. })
            )
        {
            return Err(Error::SpatialMethodMismatch);
        }
        let signal_aggregation = inputs.metric_definition.signal_aggregation();
        signal_aggregation.validate_for_spatial()?;
        if inputs.samples.len() > MAX_SAMPLES {
            return Err(Error::ResourceLimit("samples"));
        }
        inputs.samples.sort_by_key(|s| s.observation_id);
        for (i, sample) in inputs.samples.iter().enumerate() {
            if sample.floor_id != inputs.floor_id {
                return Err(Error::FloorMismatch);
            }
            if sample.frame_id != inputs.frame_id {
                return Err(Error::FrameMismatch);
            }
            if i > 0 && inputs.samples[i - 1].observation_id == sample.observation_id {
                return Err(Error::DuplicateObservation(sample.observation_id));
            }
        }
        let mut known: Vec<_> = inputs
            .samples
            .iter()
            .filter(|s| s.value.as_known().is_some())
            .collect();
        known.sort_by(|a, b| {
            a.position
                .x
                .get()
                .total_cmp(&b.position.x.get())
                .then(a.position.y.get().total_cmp(&b.position.y.get()))
                .then(a.observation_id.cmp(&b.observation_id))
        });
        let mut groups = Vec::new();
        let mut start = 0;
        while start < known.len() {
            let mut end = start + 1;
            while end < known.len() && known[end].position == known[start].position {
                end += 1;
            }
            let values: Vec<_> = known[start..end]
                .iter()
                .map(|s| kyberia_wifi_semantics::StaticSignalSample {
                    observation_id: s.observation_id,
                    rssi: *s.value.as_known().unwrap(),
                })
                .collect();
            let signal_aggregate =
                kyberia_wifi_semantics::aggregate_static(&values, signal_aggregation.method)
                    .map_err(map_aggregation_error)?;
            groups.push(LocationGroup {
                position: known[start].position,
                observation_ids: known[start..end].iter().map(|s| s.observation_id).collect(),
                signal_aggregate,
            });
            start = end;
        }
        Ok(Self {
            inputs,
            config,
            groups,
            barriers,
        })
    }
    pub fn groups(&self) -> &[LocationGroup] {
        &self.groups
    }
    pub fn inputs(&self) -> &Inputs {
        &self.inputs
    }

    pub fn barriers(&self) -> &BarrierSet {
        &self.barriers
    }

    /// Inspect the path used by barrier-aware ranking for one location group.
    /// A blocked path remains an explicit unknown while retaining the IDs of
    /// impassable barriers that caused the rejection.
    pub fn path_to_group(
        &self,
        point: Point2,
        location_group: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<PathAssessment, Error> {
        if location_group >= self.groups.len() {
            return Err(Error::InvalidConfiguration("location group index"));
        }
        if !self.barriers.is_empty()
            && self
                .groups
                .len()
                .checked_mul(self.barriers.len())
                .is_none_or(|work| work > MAX_BARRIER_EVALUATIONS)
        {
            return Err(Error::ResourceLimit("barrier evaluations"));
        }
        let trace = self.path_between(point, self.groups[location_group].position, cancelled)?;
        Ok(PathAssessment {
            location_group,
            geometric_distance: trace.geometric_distance,
            path_cost: trace.path_cost,
            crossed_barriers: trace.crossed_barriers,
            blocked_by: trace.blocked_by,
        })
    }

    /// Return bounded path diagnostics for every location group in stable
    /// group order. This is useful for an evidence drawer when a query is
    /// unknown because all nearby candidates are behind an impassable barrier.
    pub fn path_assessments(
        &self,
        point: Point2,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Vec<PathAssessment>, Error> {
        if self
            .groups
            .len()
            .checked_mul(self.barriers.len())
            .is_none_or(|work| work > MAX_BARRIER_EVALUATIONS)
        {
            return Err(Error::ResourceLimit("barrier evaluations"));
        }
        let mut assessments = Vec::with_capacity(self.groups.len());
        for index in 0..self.groups.len() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Error::Cancelled);
            }
            let trace = self.path_between(point, self.groups[index].position, cancelled)?;
            assessments.push(PathAssessment {
                location_group: index,
                geometric_distance: trace.geometric_distance,
                path_cost: trace.path_cost,
                crossed_barriers: trace.crossed_barriers,
                blocked_by: trace.blocked_by,
            });
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(assessments)
    }

    pub fn estimate(
        &self,
        point: Point2,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Cell, Error> {
        if !self.barriers.is_empty() {
            return self.estimate_barrier_aware(point, cancelled);
        }
        self.estimate_euclidean(point, cancelled)
    }

    fn estimate_euclidean(
        &self,
        point: Point2,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Cell, Error> {
        let mut nearest = BinaryHeap::new();
        let mut minimum_distance = f64::INFINITY;
        let mut support_locations = 0;
        let mut support_observations = 0;
        let mut exact = None;
        let limit = match self.config.extrapolation {
            Extrapolation::Disabled => self.config.support_radius,
            Extrapolation::WithinRadius(radius) => radius,
        }
        .get();
        for (index, group) in self.groups.iter().enumerate() {
            if index % 64 == 0 && cancelled() {
                return Err(Error::Cancelled);
            }
            // Overflow indicates a distance larger than every finite radius.
            let distance = (point.x.get() - group.position.x.get())
                .hypot(point.y.get() - group.position.y.get());
            minimum_distance = minimum_distance.min(distance);
            if distance == 0.0 {
                exact = Some(index);
            }
            if distance <= self.config.support_radius.get() {
                support_locations += 1;
                support_observations += group.observation_ids.len();
            }
            if distance.is_finite() && distance <= limit {
                nearest.push(Neighbor { distance, index });
                if nearest.len() > self.config.maximum_neighbors {
                    nearest.pop();
                }
            }
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let nearest_distance = if minimum_distance.is_finite() {
            Evidence::Known(
                Meters::new(minimum_distance).map_err(|_| Error::NumericalFailure("distance"))?,
            )
        } else {
            Evidence::Unknown(if self.groups.is_empty() {
                UnknownReason::NotMeasured
            } else {
                UnknownReason::InvalidGeometry
            })
        };
        let mut cell = Cell {
            value: Evidence::Unknown(if self.groups.is_empty() {
                UnknownReason::NotMeasured
            } else {
                UnknownReason::OutsideEvidenceSupport
            }),
            class: CellClass::Unknown,
            support_locations,
            support_observations,
            nearest_distance,
            uncertainty_db: Evidence::Unknown(UnknownReason::NotMeasured),
            contributors: Vec::new(),
        };
        if let Some(index) = exact {
            cell.value = Evidence::Known(
                *self.groups[index]
                    .signal_aggregate
                    .estimate
                    .as_known()
                    .ok_or(Error::NumericalFailure("coincident aggregate"))?,
            );
            cell.class = CellClass::Observed;
            cell.contributors.push(Contribution {
                location_group: index,
                weight: Probability::new(1.0).unwrap(),
            });
            return Ok(cell);
        }
        if matches!(self.config.method, Method::PointValue) {
            return Ok(cell);
        }
        let supported = support_locations >= self.config.minimum_locations;
        if !supported && matches!(self.config.extrapolation, Extrapolation::Disabled) {
            return Ok(cell);
        }
        let mut neighbors = nearest.into_sorted_vec();
        // Supported estimates use only supported neighbors even when extrapolation is enabled.
        if supported {
            neighbors.retain(|n| n.distance <= self.config.support_radius.get());
        }
        if neighbors.len() < self.config.minimum_locations {
            return Ok(cell);
        }
        if matches!(self.config.method, Method::Nearest) {
            neighbors.truncate(1);
        }
        let d0 = neighbors[0].distance;
        let weights: Vec<_> = neighbors
            .iter()
            .map(|n| match self.config.method {
                Method::PointValue => unreachable!("point-value returned before interpolation"),
                Method::Nearest => 1.0,
                Method::Idw { power } => (d0 / n.distance).powf(power),
            })
            .collect();
        let sum: f64 = weights.iter().sum();
        let value = convex_mean(neighbors.iter().zip(&weights).map(|(n, w)| {
            (
                self.groups[n.index]
                    .signal_aggregate
                    .estimate
                    .as_known()
                    .expect("validated coincident aggregate")
                    .get(),
                *w,
            )
        }))?;
        cell.value =
            Evidence::Known(Dbm::new(value).map_err(|_| Error::NumericalFailure("IDW result"))?);
        cell.class = if supported {
            CellClass::Interpolated
        } else {
            CellClass::Extrapolated
        };
        cell.contributors = neighbors
            .iter()
            .zip(weights)
            .filter(|(_, w)| *w > 0.0)
            .map(|(n, w)| Contribution {
                location_group: n.index,
                weight: Probability::new(w / sum).unwrap(),
            })
            .collect();
        Ok(cell)
    }

    fn estimate_barrier_aware(
        &self,
        point: Point2,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Cell, Error> {
        if self
            .groups
            .len()
            .checked_mul(self.barriers.len())
            .is_none_or(|work| work > MAX_BARRIER_EVALUATIONS)
        {
            return Err(Error::ResourceLimit("barrier evaluations"));
        }

        let mut nearest = BinaryHeap::new();
        let mut minimum_distance = f64::INFINITY;
        let mut support_locations = 0;
        let mut support_observations = 0;
        let mut exact = None;
        let limit = match self.config.extrapolation {
            Extrapolation::Disabled => self.config.support_radius,
            Extrapolation::WithinRadius(radius) => radius,
        }
        .get();
        for (index, group) in self.groups.iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Error::Cancelled);
            }
            if group.position == point {
                exact = Some(index);
            }
            let trace = self.path_between(point, group.position, cancelled)?;
            let Some(path) = trace.path_cost.as_known() else {
                // A disallowed barrier makes this source unavailable. Its
                // geometric distance is intentionally not counted as support.
                continue;
            };
            let distance = path.total_cost.get();
            minimum_distance = minimum_distance.min(distance);
            if distance <= self.config.support_radius.get() {
                support_locations += 1;
                support_observations += group.observation_ids.len();
            }
            if distance.is_finite() && distance <= limit {
                let selection_score = match self.config.method {
                    Method::Idw { power } => {
                        let attenuation_factor = 10.0_f64.powf(-path.attenuation_db.get() / 10.0);
                        distance / attenuation_factor.powf(1.0 / power)
                    }
                    Method::Nearest | Method::PointValue => distance,
                };
                nearest.push(BarrierNeighbor {
                    distance,
                    selection_score,
                    path: path.clone(),
                    index,
                });
                if nearest.len() > self.config.maximum_neighbors {
                    nearest.pop();
                }
            }
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let nearest_distance = if minimum_distance.is_finite() {
            Evidence::Known(
                Meters::new(minimum_distance)
                    .map_err(|_| Error::NumericalFailure("path distance"))?,
            )
        } else {
            Evidence::Unknown(if self.groups.is_empty() {
                UnknownReason::NotMeasured
            } else {
                UnknownReason::OutsideEvidenceSupport
            })
        };
        let mut cell = Cell {
            value: Evidence::Unknown(if self.groups.is_empty() {
                UnknownReason::NotMeasured
            } else {
                UnknownReason::OutsideEvidenceSupport
            }),
            class: CellClass::Unknown,
            support_locations,
            support_observations,
            nearest_distance,
            uncertainty_db: Evidence::Unknown(UnknownReason::NotMeasured),
            contributors: Vec::new(),
        };
        if let Some(index) = exact {
            cell.value = Evidence::Known(
                *self.groups[index]
                    .signal_aggregate
                    .estimate
                    .as_known()
                    .ok_or(Error::NumericalFailure("coincident aggregate"))?,
            );
            cell.class = CellClass::Observed;
            cell.contributors.push(Contribution {
                location_group: index,
                weight: Probability::new(1.0).unwrap(),
            });
            return Ok(cell);
        }
        if matches!(self.config.method, Method::PointValue) {
            return Ok(cell);
        }
        let supported = support_locations >= self.config.minimum_locations;
        if !supported && matches!(self.config.extrapolation, Extrapolation::Disabled) {
            return Ok(cell);
        }
        let mut neighbors = nearest.into_sorted_vec();
        if supported {
            neighbors.retain(|neighbor| neighbor.distance <= self.config.support_radius.get());
        }
        if neighbors.len() < self.config.minimum_locations {
            return Ok(cell);
        }
        if matches!(self.config.method, Method::Nearest) {
            neighbors.truncate(1);
        }
        let d0 = neighbors[0].distance;
        let mut weighted: Vec<_> = neighbors
            .iter()
            .map(|neighbor| match self.config.method {
                Method::PointValue => unreachable!("point-value returned before interpolation"),
                Method::Nearest => 1.0,
                Method::Idw { power } => {
                    // Path length affects spatial proximity; material loss is
                    // applied independently in linear power so a high-loss
                    // wall cannot retain the influence of an open path at the
                    // same geometric distance.
                    let distance_weight = (d0 / neighbor.distance).powf(power);
                    let attenuation_weight =
                        10.0_f64.powf(-neighbor.path.attenuation_db.get() / 10.0);
                    distance_weight * attenuation_weight
                }
            })
            .zip(neighbors.iter())
            .collect();
        // Surface the strongest effective contributors first. For an
        // unattenuated model this remains nearest-first; with a wall, the
        // lower-loss path can outrank a geometrically closer sample.
        weighted.sort_by(|(left, _), (right, _)| {
            right
                .total_cmp(left)
                .then_with(|| std::cmp::Ordering::Equal)
        });
        let weights: Vec<_> = weighted.iter().map(|(weight, _)| *weight).collect();
        let neighbors: Vec<_> = weighted.into_iter().map(|(_, neighbor)| neighbor).collect();
        let sum: f64 = weights.iter().sum();
        if !sum.is_finite() || sum <= 0.0 {
            return Err(Error::NumericalFailure("barrier IDW weights"));
        }
        let value = convex_mean(neighbors.iter().zip(&weights).map(|(neighbor, weight)| {
            (
                self.groups[neighbor.index]
                    .signal_aggregate
                    .estimate
                    .as_known()
                    .expect("validated coincident aggregate")
                    .get(),
                *weight,
            )
        }))?;
        cell.value = Evidence::Known(
            Dbm::new(value).map_err(|_| Error::NumericalFailure("barrier IDW result"))?,
        );
        cell.class = if supported {
            CellClass::Interpolated
        } else {
            CellClass::Extrapolated
        };
        cell.contributors = neighbors
            .iter()
            .zip(weights)
            .filter(|(_, weight)| *weight > 0.0)
            .map(|(neighbor, weight)| Contribution {
                location_group: neighbor.index,
                weight: Probability::new(weight / sum).unwrap(),
            })
            .collect();
        Ok(cell)
    }

    fn path_between(
        &self,
        from: Point2,
        to: Point2,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<PathTrace, Error> {
        let geometric_distance = (from.x.get() - to.x.get()).hypot(from.y.get() - to.y.get());
        if !geometric_distance.is_finite() {
            return Ok(PathTrace {
                geometric_distance: Evidence::Unknown(UnknownReason::InvalidGeometry),
                path_cost: Evidence::Unknown(UnknownReason::InvalidGeometry),
                crossed_barriers: Vec::new(),
                blocked_by: Vec::new(),
            });
        }
        if geometric_distance == 0.0 {
            let path = PathCost::new(0.0, 0.0, 0.0, Vec::new())?;
            return Ok(PathTrace {
                geometric_distance: Evidence::Known(path.geometric_distance),
                path_cost: Evidence::Known(path),
                crossed_barriers: Vec::new(),
                blocked_by: Vec::new(),
            });
        }
        let mut crossed_barriers = Vec::new();
        let mut blocked_by = Vec::new();
        let mut traversal_cost = 0.0;
        let mut attenuation_db = 0.0;
        for (index, barrier) in self.barriers.as_slice().iter().enumerate() {
            if index.is_multiple_of(64) && cancelled() {
                return Err(Error::Cancelled);
            }
            if !segments_intersect(from, to, barrier.start, barrier.end) {
                continue;
            }
            crossed_barriers.push(barrier.id);
            if barrier.material.policy == BarrierPolicy::Impassable {
                blocked_by.push(barrier.id);
            }
            traversal_cost += barrier.material.traversal_cost.get();
            attenuation_db += barrier.material.attenuation_db.get();
        }
        if !blocked_by.is_empty() {
            return Ok(PathTrace {
                geometric_distance: Evidence::Known(
                    Meters::new(geometric_distance)
                        .map_err(|_| Error::NumericalFailure("path distance"))?,
                ),
                path_cost: Evidence::Unknown(UnknownReason::OutsideEvidenceSupport),
                crossed_barriers,
                blocked_by,
            });
        }
        let path = PathCost::new(
            geometric_distance,
            traversal_cost,
            attenuation_db,
            crossed_barriers,
        )?;
        let crossed_barriers = path.crossed_barriers.clone();
        Ok(PathTrace {
            geometric_distance: Evidence::Known(path.geometric_distance),
            path_cost: Evidence::Known(path),
            crossed_barriers,
            blocked_by: Vec::new(),
        })
    }
}

fn map_aggregation_error(error: kyberia_wifi_semantics::Error) -> Error {
    match error {
        kyberia_wifi_semantics::Error::InvalidConfiguration(reason) => {
            Error::InvalidAggregationConfiguration(reason)
        }
        kyberia_wifi_semantics::Error::DuplicateObservation(observation_id) => {
            Error::DuplicateObservation(observation_id)
        }
        kyberia_wifi_semantics::Error::ClockEpochMismatch => {
            Error::InvalidAggregationConfiguration("clock epoch mismatch in spatial aggregation")
        }
        kyberia_wifi_semantics::Error::NonMonotonicSequence => {
            Error::InvalidAggregationConfiguration("non-monotonic sequence in spatial aggregation")
        }
        kyberia_wifi_semantics::Error::TemporalMethodRequiresMonotonicTime => {
            Error::TemporalAggregationRequiresMonotonicEvidence
        }
        kyberia_wifi_semantics::Error::ResourceLimit => {
            Error::ResourceLimit("signal aggregation samples")
        }
        kyberia_wifi_semantics::Error::NumericalFailure => {
            Error::NumericalFailure("signal aggregation")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum CellClass {
    Observed,
    Interpolated,
    Extrapolated,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Contribution {
    pub location_group: usize,
    pub weight: Probability,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Cell {
    pub value: Evidence<Dbm>,
    pub class: CellClass,
    pub support_locations: usize,
    pub support_observations: usize,
    pub nearest_distance: Evidence<Meters>,
    /// No calibrated uncertainty is inferred from neighbor spread or density.
    pub uncertainty_db: Evidence<Db>,
    pub contributors: Vec<Contribution>,
}
#[derive(Clone, Copy, Debug)]
struct Neighbor {
    distance: f64,
    index: usize,
}

#[derive(Clone, Debug)]
struct BarrierNeighbor {
    distance: f64,
    selection_score: f64,
    path: PathCost,
    index: usize,
}
struct PathTrace {
    geometric_distance: Evidence<Meters>,
    path_cost: Evidence<PathCost>,
    crossed_barriers: Vec<BarrierId>,
    blocked_by: Vec<BarrierId>,
}
impl PartialEq for BarrierNeighbor {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for BarrierNeighbor {}
impl PartialOrd for BarrierNeighbor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for BarrierNeighbor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.selection_score
            .total_cmp(&other.selection_score)
            .then(self.distance.total_cmp(&other.distance))
            .then(self.index.cmp(&other.index))
    }
}
impl PartialEq for Neighbor {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Neighbor {}
impl PartialOrd for Neighbor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Neighbor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.index.cmp(&other.index))
    }
}

/// Scaled compensated weighted mean avoids overflow, including opposite extrema.
fn convex_mean(values: impl Iterator<Item = (f64, f64)> + Clone) -> Result<f64, Error> {
    let scale = values.clone().fold(0.0_f64, |a, (v, _)| a.max(v.abs()));
    if scale == 0.0 {
        return Ok(0.0);
    }
    let mut numerator = 0.0;
    let mut correction = 0.0;
    let mut denominator = 0.0;
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for (value, weight) in values {
        minimum = minimum.min(value);
        maximum = maximum.max(value);
        let adjusted = (value / scale) * weight - correction;
        let next = numerator + adjusted;
        correction = (next - numerator) - adjusted;
        numerator = next;
        denominator += weight;
    }
    let value = (numerator / denominator).clamp(-1.0, 1.0) * scale;
    if !value.is_finite() {
        return Err(Error::NumericalFailure("weighted mean"));
    }
    Ok(value.clamp(minimum, maximum))
}
