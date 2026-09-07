//! Unknown is a reason, not zero or an omitted/null field.
use crate::identity::{ContentHash, Text};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum Evidence<T> {
    Known(T),
    Unknown(UnknownReason),
}
impl<T> Evidence<T> {
    pub fn as_known(&self) -> Option<&T> {
        match self {
            Self::Known(v) => Some(v),
            Self::Unknown(_) => None,
        }
    }
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Evidence<U> {
        match self {
            Self::Known(v) => Evidence::Known(f(v)),
            Self::Unknown(r) => Evidence::Unknown(r),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    NotMeasured,
    NotAdvertised,
    NotObservable,
    NotApplicable,
    UnsupportedCapability,
    PermissionDenied,
    FilteredOut,
    BelowDetectionThreshold,
    FailedTest,
    NoAssociation,
    InvalidGeometry,
    SolverFailure,
    OutsideEvidenceSupport,
    ClockUnavailable,
    SourceDidNotProvide,
    Redacted,
    NotRetained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    Observed,
    Inferred,
    Interpolated,
    Extrapolated,
    Simulated,
    Calibrated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReference {
    pub sha256: ContentHash,
    pub media_type: Text,
    pub byte_length: u64,
}

/// A closed version tag rejects unknown semantic schema versions. Additive
/// object fields are ignored; producers must bump version for semantic changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaVersion {
    #[serde(rename = "1")]
    V1,
}
