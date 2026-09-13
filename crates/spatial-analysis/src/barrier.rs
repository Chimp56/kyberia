use crate::{Error, Point2};
use kyberia_domain::{
    identity::ContentHash,
    units::{Db, Meters},
};
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use std::fmt;

/// Maximum number of ordered barrier segments retained by one model.
pub const MAX_BARRIERS: usize = 4_096;
/// Coordinate bound for the inward planar barrier contract. It bounds
/// orientation arithmetic; it is not a projection or snapping tolerance.
pub const MAX_BARRIER_COORDINATE: f64 = 1.0e9;
/// A bounded upper limit for a material's path-cost surcharge.
pub const MAX_BARRIER_TRAVERSAL_COST_M: f64 = 1.0e6;
/// A bounded nonnegative heuristic influence prior. Keeping this finite also
/// bounds the linear-power factor used by barrier-aware IDW.
pub const MAX_BARRIER_ATTENUATION_DB: f64 = 300.0;
/// A model may not perform more barrier tests than this in one query.
pub const MAX_BARRIER_EVALUATIONS: usize = 100_000_000;

/// Stable identity for a barrier and its material policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct BarrierId(u32);

impl BarrierId {
    pub fn new(value: u32) -> Result<Self, Error> {
        if value == 0 {
            return Err(Error::InvalidBarrier("barrier IDs must be nonzero"));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for BarrierId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(|error| de::Error::custom(error.to_string()))
    }
}

/// Whether a crossing contributes finite loss or makes that source/query
/// relationship unavailable. An impassable barrier is a hard support boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarrierPolicy {
    Passable,
    Impassable,
}

/// Material semantics are deliberately finite and typed. Traversal cost is a
/// geometric surcharge in meters; attenuation is a nonnegative heuristic IDW
/// influence prior in dB, not a physical per-path signal attenuation model.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BarrierMaterial {
    pub traversal_cost: Meters,
    pub attenuation_db: Db,
    pub policy: BarrierPolicy,
}

impl<'de> Deserialize<'de> for BarrierMaterial {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            traversal_cost: Meters,
            attenuation_db: Db,
            policy: BarrierPolicy,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.traversal_cost, wire.attenuation_db, wire.policy)
            .map_err(|error| de::Error::custom(error.to_string()))
    }
}

impl BarrierMaterial {
    pub fn new(
        traversal_cost: Meters,
        attenuation_db: Db,
        policy: BarrierPolicy,
    ) -> Result<Self, Error> {
        if traversal_cost.get() > MAX_BARRIER_TRAVERSAL_COST_M {
            return Err(Error::InvalidBarrier("traversal cost exceeds bound"));
        }
        if attenuation_db.get() < 0.0 || attenuation_db.get() > MAX_BARRIER_ATTENUATION_DB {
            return Err(Error::InvalidBarrier("attenuation must be in [0, 300] dB"));
        }
        Ok(Self {
            traversal_cost,
            attenuation_db,
            policy,
        })
    }
}

/// A finite planar segment in one already-resolved floor/frame. Foreign
/// geometry objects do not cross this crate boundary.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BarrierSegment {
    pub id: BarrierId,
    pub start: Point2,
    pub end: Point2,
    pub material: BarrierMaterial,
}

impl<'de> Deserialize<'de> for BarrierSegment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            id: BarrierId,
            #[serde(deserialize_with = "deserialize_point")]
            start: Point2,
            #[serde(deserialize_with = "deserialize_point")]
            end: Point2,
            material: BarrierMaterial,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.id, wire.start, wire.end, wire.material)
            .map_err(|error| de::Error::custom(error.to_string()))
    }
}

impl BarrierSegment {
    pub fn new(
        id: BarrierId,
        start: Point2,
        end: Point2,
        material: BarrierMaterial,
    ) -> Result<Self, Error> {
        if !coordinate_in_range(start) || !coordinate_in_range(end) {
            return Err(Error::InvalidBarrier("barrier coordinate exceeds bound"));
        }
        if start == end {
            return Err(Error::InvalidBarrier("degenerate barrier segment"));
        }
        // Revalidate copied material values so a future constructor or
        // deserializer cannot bypass the finite semantic limits.
        BarrierMaterial::new(
            material.traversal_cost,
            material.attenuation_db,
            material.policy,
        )?;
        Ok(Self {
            id,
            start,
            end,
            material,
        })
    }
}

/// Canonically ordered barrier collection. Ordering by stable ID means barrier
/// and input permutations produce identical path traces and serialized bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BarrierSet {
    barriers: Vec<BarrierSegment>,
}

impl BarrierSet {
    pub fn new(mut barriers: Vec<BarrierSegment>) -> Result<Self, Error> {
        if barriers.len() > MAX_BARRIERS {
            return Err(Error::ResourceLimit("barriers"));
        }
        barriers.sort_by_key(|barrier| barrier.id);
        if barriers
            .windows(2)
            .any(|window| window[0].id == window[1].id)
        {
            return Err(Error::InvalidBarrier("duplicate barrier ID"));
        }
        for barrier in &barriers {
            BarrierSegment::new(barrier.id, barrier.start, barrier.end, barrier.material)?;
        }
        Ok(Self { barriers })
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn as_slice(&self) -> &[BarrierSegment] {
        &self.barriers
    }

    pub fn is_empty(&self) -> bool {
        self.barriers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.barriers.len()
    }

    /// Stable canonical bytes for a barrier artifact or analysis identity.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec(self).map_err(|_| Error::NumericalFailure("barrier serialization"))
    }

    pub fn content_hash(&self) -> Result<ContentHash, Error> {
        Ok(ContentHash::from_sha256(
            Sha256::digest(self.canonical_bytes()?).into(),
        ))
    }
}

impl<'de> Deserialize<'de> for BarrierSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "deserialize_barriers")]
            barriers: Vec<BarrierSegment>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.barriers).map_err(|error| de::Error::custom(error.to_string()))
    }
}

fn coordinate_in_range(point: Point2) -> bool {
    point.x.get().abs() <= MAX_BARRIER_COORDINATE && point.y.get().abs() <= MAX_BARRIER_COORDINATE
}

/// Effective direct path evidence for one source/query pair. The geometric
/// distance is retained separately from material traversal cost and the
/// heuristic influence prior; callers can audit exactly why a contributor was
/// ranked lower.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathCost {
    pub geometric_distance: Meters,
    pub traversal_cost: Meters,
    pub total_cost: Meters,
    pub attenuation_db: Db,
    pub crossed_barriers: Vec<BarrierId>,
}

/// A path inspection result. `path_cost` is unknown for at least one
/// impassable crossed barrier; `blocked_by` remains populated for diagnosis.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathAssessment {
    pub location_group: usize,
    pub geometric_distance: kyberia_domain::evidence::Evidence<Meters>,
    pub path_cost: kyberia_domain::evidence::Evidence<PathCost>,
    /// Every barrier touched by the direct path, including passable barriers
    /// before an impassable barrier rejects the relationship.
    pub crossed_barriers: Vec<BarrierId>,
    pub blocked_by: Vec<BarrierId>,
}

impl PathCost {
    pub(crate) fn new(
        geometric_distance: f64,
        traversal_cost: f64,
        attenuation_db: f64,
        crossed_barriers: Vec<BarrierId>,
    ) -> Result<Self, Error> {
        let total_cost = geometric_distance + traversal_cost;
        if !total_cost.is_finite() {
            return Err(Error::NumericalFailure("path cost"));
        }
        Ok(Self {
            geometric_distance: Meters::new(geometric_distance)
                .map_err(|_| Error::NumericalFailure("path distance"))?,
            traversal_cost: Meters::new(traversal_cost)
                .map_err(|_| Error::NumericalFailure("barrier traversal cost"))?,
            total_cost: Meters::new(total_cost)
                .map_err(|_| Error::NumericalFailure("total path cost"))?,
            attenuation_db: Db::new(attenuation_db)
                .map_err(|_| Error::NumericalFailure("barrier attenuation"))?,
            crossed_barriers,
        })
    }
}

pub(crate) fn segments_intersect(
    a: Point2,
    b: Point2,
    c: Point2,
    d: Point2,
) -> Result<bool, Error> {
    if ![
        a.x.get(),
        a.y.get(),
        b.x.get(),
        b.y.get(),
        c.x.get(),
        c.y.get(),
        d.x.get(),
        d.y.get(),
    ]
    .iter()
    .all(|value| value.is_finite())
    {
        return Err(Error::NumericalFailure("barrier intersection unresolved"));
    }
    let first = orientation_sign(a, b, c)?;
    let second = orientation_sign(a, b, d)?;
    let third = orientation_sign(c, d, a)?;
    let fourth = orientation_sign(c, d, b)?;
    if first == 0 && on_segment(a, b, c)
        || second == 0 && on_segment(a, b, d)
        || third == 0 && on_segment(c, d, a)
        || fourth == 0 && on_segment(c, d, b)
    {
        return Ok(true);
    }
    Ok(first != second && third != fourth)
}

/// Return an adaptive orientation sign. Relative coordinates are scaled before
/// multiplication, and the determinant gets a data-dependent roundoff bound.
/// A determinant inside that bound is an explicit numerical failure; it must
/// never be silently interpreted as a disjoint segment.
fn orientation_sign(a: Point2, b: Point2, c: Point2) -> Result<i8, Error> {
    let ux = b.x.get() - a.x.get();
    let uy = b.y.get() - a.y.get();
    let vx = c.x.get() - a.x.get();
    let vy = c.y.get() - a.y.get();
    let scale = ux.abs().max(uy.abs()).max(vx.abs()).max(vy.abs());
    if !scale.is_finite() {
        return Err(Error::NumericalFailure("barrier intersection unresolved"));
    }
    if scale == 0.0 {
        return Ok(0);
    }
    let ux_scaled = ux / scale;
    let uy_scaled = uy / scale;
    let vx_scaled = vx / scale;
    let vy_scaled = vy / scale;
    if (ux != 0.0 && ux_scaled == 0.0)
        || (uy != 0.0 && uy_scaled == 0.0)
        || (vx != 0.0 && vx_scaled == 0.0)
        || (vy != 0.0 && vy_scaled == 0.0)
    {
        return Err(Error::NumericalFailure("barrier intersection unresolved"));
    }
    let left = ux_scaled * vy_scaled;
    let right = uy_scaled * vx_scaled;
    if (ux_scaled != 0.0 && vy_scaled != 0.0 && left == 0.0)
        || (uy_scaled != 0.0 && vx_scaled != 0.0 && right == 0.0)
        || (left != 0.0 && left.abs() < f64::MIN_POSITIVE)
        || (right != 0.0 && right.abs() < f64::MIN_POSITIVE)
    {
        return Err(Error::NumericalFailure("barrier intersection unresolved"));
    }
    let determinant = ux_scaled.mul_add(vy_scaled, -right);
    let error_bound = 8.0 * f64::EPSILON * (left.abs() + right.abs());
    if !determinant.is_finite() || !error_bound.is_finite() {
        return Err(Error::NumericalFailure("barrier intersection unresolved"));
    }
    if determinant.abs() > error_bound {
        return Ok(if determinant > 0.0 { 1 } else { -1 });
    }

    // A rounded determinant inside its error bound gets an exact expansion of
    // the two normalized products. This resolves true diagonal collinearity
    // while preserving an explicit error for products that underflowed.
    let (left_hi, left_lo) = two_product(ux_scaled, vy_scaled);
    let (right_hi, right_lo) = two_product(uy_scaled, vx_scaled);
    let (x3, x2, x1, x0) = two_two_diff(left_hi, left_lo, right_hi, right_lo);
    for component in [x3, x2, x1, x0] {
        if component != 0.0 {
            return Ok(if component > 0.0 { 1 } else { -1 });
        }
    }
    Ok(0)
}

fn on_segment(a: Point2, b: Point2, point: Point2) -> bool {
    point.x.get() >= a.x.get().min(b.x.get())
        && point.x.get() <= a.x.get().max(b.x.get())
        && point.y.get() >= a.y.get().min(b.y.get())
        && point.y.get() <= a.y.get().max(b.y.get())
}

const SPLITTER: f64 = 134_217_729.0;

#[inline]
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let product = a * b;
    let (ahi, alo) = split(a);
    let (bhi, blo) = split(b);
    let err1 = product - ahi * bhi;
    let err2 = err1 - alo * bhi;
    let err3 = err2 - ahi * blo;
    (product, alo * blo - err3)
}

#[inline]
fn split(value: f64) -> (f64, f64) {
    let scaled = SPLITTER * value;
    let high = scaled - value;
    let high = scaled - high;
    (high, value - high)
}

#[inline]
fn two_two_diff(a1: f64, a0: f64, b1: f64, b0: f64) -> (f64, f64, f64, f64) {
    let (intermediate, remainder, x0) = two_one_diff(a1, a0, b0);
    let (x3, x2, low) = two_one_diff(intermediate, remainder, b1);
    let (x1, x0) = two_sum(low, x0);
    (x3, x2, x1, x0)
}

#[inline]
fn two_one_diff(a1: f64, a0: f64, b: f64) -> (f64, f64, f64) {
    let (intermediate, x0) = two_diff(a0, b);
    let (x2, x1) = two_sum(a1, intermediate);
    (x2, x1, x0)
}

#[inline]
fn two_diff(a: f64, b: f64) -> (f64, f64) {
    let difference = a - b;
    let bvirt = a - difference;
    let avirt = difference + bvirt;
    let bround = bvirt - b;
    let around = a - avirt;
    (difference, around + bround)
}

#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let sum = a + b;
    let bvirt = sum - a;
    let avirt = sum - bvirt;
    let bround = b - bvirt;
    let around = a - avirt;
    (sum, around + bround)
}

fn deserialize_point<'de, D>(deserializer: D) -> Result<Point2, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct WirePoint {
        x: kyberia_domain::units::CoordinateMeters,
        y: kyberia_domain::units::CoordinateMeters,
    }
    let point = WirePoint::deserialize(deserializer)?;
    Ok(Point2 {
        x: point.x,
        y: point.y,
    })
}

fn deserialize_barriers<'de, D>(deserializer: D) -> Result<Vec<BarrierSegment>, D::Error>
where
    D: Deserializer<'de>,
{
    struct BarrierVisitor;
    impl<'de> de::Visitor<'de> for BarrierVisitor {
        type Value = Vec<BarrierSegment>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded array of barrier segments")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let hint = sequence.size_hint().unwrap_or(0);
            if hint > MAX_BARRIERS {
                return Err(de::Error::custom("barrier count exceeds bound"));
            }
            let mut barriers = Vec::with_capacity(hint);
            while let Some(barrier) = sequence.next_element()? {
                if barriers.len() == MAX_BARRIERS {
                    return Err(de::Error::custom("barrier count exceeds bound"));
                }
                barriers.push(barrier);
            }
            Ok(barriers)
        }
    }
    deserializer.deserialize_seq(BarrierVisitor)
}
