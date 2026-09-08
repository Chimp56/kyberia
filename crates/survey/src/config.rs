use crate::*;
use kyberia_domain::capability::{Capability, CapabilityDocument};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::num::NonZeroU32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointId([u8; 16]);
impl PointId {
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, kyberia_domain::ValidationError> {
        if bytes == [0; 16] {
            return Err(kyberia_domain::ValidationError::InvalidIdentity);
        }
        Ok(Self(bytes))
    }

    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }

    /// Stable, lossless encoding for SQLite identity columns. This named
    /// boundary keeps PointId distinct from all other domain identifiers and
    /// avoids exposing a generic `TryFrom<String>` database conversion.
    pub fn database_key(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_database_key(value: &str) -> Result<Self, kyberia_domain::ValidationError> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(kyberia_domain::ValidationError::InvalidIdentity);
        }
        let mut bytes = [0; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| kyberia_domain::ValidationError::InvalidIdentity)?;
        }
        Self::from_bytes(bytes)
    }
}

impl Serialize for PointId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.database_key())
    }
}

impl<'de> Deserialize<'de> for PointId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_database_key(&value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod point_id_tests {
    use super::PointId;

    #[test]
    fn database_key_roundtrips_without_identity_type_erasure() {
        let point = PointId::from_bytes([0x12; 16]).unwrap();
        let key = point.database_key();
        assert_eq!(key, "12121212121212121212121212121212");
        assert_eq!(PointId::from_database_key(&key), Ok(point));
        assert_eq!(serde_json::to_string(&point).unwrap(), format!("\"{key}\""));
        assert_eq!(
            serde_json::from_str::<PointId>(&format!("\"{key}\"")).unwrap(),
            point
        );
    }

    #[test]
    fn malformed_database_keys_are_rejected() {
        for key in [
            "",
            "1212",
            "1212121212121212121212121212121G",
            "ABCDEFABCDEFABCDEFABCDEFABCDEFAB",
            "00000000000000000000000000000000",
        ] {
            assert!(PointId::from_database_key(key).is_err(), "accepted {key:?}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    Scan,
    Frame,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointMetric {
    Rssi,
    Noise,
    Snr,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelRequirement {
    pub frequency: Hertz,
    pub minimum_dwell: Seconds,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "bssid", rename_all = "snake_case")]
pub enum Target {
    AnyBssid,
    Bssid(MacAddress),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PosePolicy {
    ManualAnchor {
        maximum_reported_offset: Meters,
    },
    RequireReported {
        maximum_offset: Meters,
        maximum_axis_stddev: Meters,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointConfigData {
    pub schema_version: SchemaVersion,
    pub point_id: PointId,
    pub session_id: SessionId,
    pub anchor: PoseReference,
    pub map_calibration: Evidence<CalibrationId>,
    pub source_id: SourceId,
    pub collector_id: CollectorId,
    pub adapter_version: Text,
    pub epoch: ClockEpochId,
    pub capabilities: CapabilityDocument,
    pub mode: CaptureMode,
    pub required_capabilities: Vec<Capability>,
    #[serde(deserialize_with = "metric_requirements")]
    pub metrics: BTreeMap<PointMetric, NonZeroU32>,
    pub channels: Vec<ChannelRequirement>,
    pub minimum_active_time: Seconds,
    /// Maximum age since actual capture at admission, including cache and queue time.
    pub maximum_scan_age: Seconds,
    pub target: Target,
    pub pose_policy: PosePolicy,
    /// Explicit fixture/replay mode; never enabled implicitly by source kind.
    pub allow_synthetic: bool,
    pub method_version: Text,
}
fn metric_requirements<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PointMetric, NonZeroU32>, D::Error> {
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = BTreeMap<PointMetric, NonZeroU32>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("unique point metric requirements")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut metrics = BTreeMap::new();
            while let Some((metric, count)) = map.next_entry()? {
                if metrics.insert(metric, count).is_some() {
                    return Err(serde::de::Error::custom("duplicate point metric"));
                }
            }
            Ok(metrics)
        }
    }
    deserializer.deserialize_map(Visitor)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PointConfigData", into = "PointConfigData")]
pub struct PointConfig(PointConfigData);
impl PointConfig {
    pub fn new(data: PointConfigData) -> Result<Self, SurveyError> {
        if !data.metrics.contains_key(&PointMetric::Rssi)
            || data
                .metrics
                .values()
                .any(|v| v.get() as usize > MAX_RECORDS)
            || data.channels.len() > 256
            || data.required_capabilities.len() > 32
        {
            return Err(SurveyError::InvalidConfiguration);
        }
        if data.capabilities.collector_id != data.collector_id
            || data.capabilities.collector_version != data.adapter_version
        {
            return Err(SurveyError::InvalidConfiguration);
        }
        for (i, channel) in data.channels.iter().enumerate() {
            if channel.minimum_dwell.get() == 0.0
                || data.channels[..i]
                    .iter()
                    .any(|c| c.frequency == channel.frequency)
            {
                return Err(SurveyError::InvalidConfiguration);
            }
            nanos(channel.minimum_dwell)?;
        }
        nanos(data.minimum_active_time)?;
        nanos(data.maximum_scan_age)?;
        let config = Self(data);
        if let PosePolicy::RequireReported {
            maximum_axis_stddev,
            ..
        } = config.0.pose_policy
        {
            check_covariance(&config.0.anchor, maximum_axis_stddev)?;
        }
        Ok(config)
    }
    pub const fn data(&self) -> &PointConfigData {
        &self.0
    }
    pub(crate) fn preflight(&self) -> Result<(), SurveyError> {
        let mut required = self.0.required_capabilities.clone();
        required.push(match self.0.mode {
            CaptureMode::Scan => Capability::NearbyScan,
            CaptureMode::Frame => Capability::MonitorFrames,
        });
        if self.0.metrics.contains_key(&PointMetric::Noise)
            || self.0.metrics.contains_key(&PointMetric::Snr)
        {
            required.push(Capability::NoiseDbm);
        }
        let missing: Vec<_> = required
            .into_iter()
            .filter(|c| {
                self.0
                    .capabilities
                    .require_available(std::slice::from_ref(c))
                    .is_err()
            })
            .collect();
        if !missing.is_empty() {
            return Err(SurveyError::UnsupportedCapabilities(missing));
        }
        Ok(())
    }
    pub(crate) fn check_pose(&self, pose: &Evidence<PoseReference>) -> Result<(), SurveyError> {
        let Some(pose) = pose.as_known() else {
            return if matches!(self.0.pose_policy, PosePolicy::ManualAnchor { .. }) {
                Ok(())
            } else {
                Err(SurveyError::PoseUnavailable)
            };
        };
        if pose.frame_id != self.0.anchor.frame_id {
            return Err(SurveyError::WrongFrame);
        }
        let distance = (pose.position.x.get() - self.0.anchor.position.x.get())
            .hypot(pose.position.y.get() - self.0.anchor.position.y.get())
            .hypot(pose.position.z.get() - self.0.anchor.position.z.get());
        let maximum = match self.0.pose_policy {
            PosePolicy::ManualAnchor {
                maximum_reported_offset,
            } => maximum_reported_offset,
            PosePolicy::RequireReported {
                maximum_offset,
                maximum_axis_stddev,
            } => {
                check_covariance(pose, maximum_axis_stddev)?;
                maximum_offset
            }
        };
        if !distance.is_finite() || distance > maximum.get() {
            return Err(SurveyError::PoseOutsidePoint);
        }
        Ok(())
    }
}
fn check_covariance(pose: &PoseReference, maximum: Meters) -> Result<(), SurveyError> {
    let covariance = pose
        .covariance
        .as_known()
        .ok_or(SurveyError::PoseUnavailable)?
        .packed();
    if [covariance[0], covariance[3], covariance[5]]
        .into_iter()
        .any(|v| v.sqrt() > maximum.get())
    {
        return Err(SurveyError::PoseUncertain);
    }
    Ok(())
}
impl TryFrom<PointConfigData> for PointConfig {
    type Error = SurveyError;
    fn try_from(v: PointConfigData) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}
impl From<PointConfig> for PointConfigData {
    fn from(v: PointConfig) -> Self {
        v.0
    }
}
