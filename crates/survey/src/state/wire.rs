use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PointSnapshotInputVersion {
    LegacyUntaggedV1,
    #[serde(rename = "2")]
    V2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PointSnapshotDecodeReceipt {
    pub input_schema_version: PointSnapshotInputVersion,
    pub output_schema_version: PointSnapshotSchemaVersion,
    pub decoder_version: &'static str,
    pub migrated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedPointSurvey {
    pub survey: PointSurvey,
    pub receipt: PointSnapshotDecodeReceipt,
}

#[derive(Default, Deserialize)]
enum WireTag {
    // Only a missing field selects the historical untagged wire shape. Explicit
    // null, "1", unknown strings, and an Evidence value in legacy records fail.
    #[default]
    #[serde(skip)]
    LegacyUntaggedV1,
    #[serde(rename = "2")]
    V2,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum VersionValue {
    Legacy(Text),
    Current(Evidence<Text>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotWire {
    #[serde(default)]
    schema_version: WireTag,
    config: PointConfig,
    phase: PointPhase,
    started: u64,
    last: u64,
    windows: Vec<ActiveWindow>,
    records: Vec<AcceptedEvidence<VersionValue>>,
}

impl<'de> Deserialize<'de> for DecodedPointSurvey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SnapshotWire::deserialize(deserializer)?;
        if wire.records.len() > MAX_RECORDS || wire.windows.len() > MAX_WINDOWS {
            return Err(serde::de::Error::custom("point receipt resource limit"));
        }
        let input = match wire.schema_version {
            WireTag::LegacyUntaggedV1 => PointSnapshotInputVersion::LegacyUntaggedV1,
            WireTag::V2 => PointSnapshotInputVersion::V2,
        };
        let records: Result<Vec<_>, D::Error> = wire
            .records
            .into_iter()
            .map(|record| {
                let source_version = match (&wire.schema_version, record.source_version) {
                    (WireTag::LegacyUntaggedV1, VersionValue::Legacy(version)) => {
                        Evidence::Known(version)
                    }
                    (WireTag::V2, VersionValue::Current(version)) => version,
                    _ => {
                        return Err(serde::de::Error::custom(
                            "source version shape disagrees with point snapshot schema",
                        ));
                    }
                };
                Ok(AcceptedEvidence {
                    observation_id: record.observation_id,
                    captured: record.captured,
                    result_age: record.result_age,
                    admitted: record.admitted,
                    bssid: record.bssid,
                    rssi: record.rssi,
                    noise: record.noise,
                    pose: record.pose,
                    calibration: record.calibration,
                    raw_source: record.raw_source,
                    source_version,
                    parser_version: record.parser_version,
                    quality: record.quality,
                    dwell: record.dwell,
                    tuned_frequency: record.tuned_frequency,
                })
            })
            .collect();
        let survey = PointSurvey::try_from(Snapshot {
            schema_version: PointSnapshotSchemaVersion::V2,
            config: wire.config,
            phase: wire.phase,
            started: wire.started,
            last: wire.last,
            windows: wire.windows,
            records: records?,
        })
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            survey,
            receipt: PointSnapshotDecodeReceipt {
                input_schema_version: input,
                output_schema_version: PointSnapshotSchemaVersion::V2,
                decoder_version: "kyberia-point-snapshot/2.0.0",
                migrated: input == PointSnapshotInputVersion::LegacyUntaggedV1,
            },
        })
    }
}

impl<'de> Deserialize<'de> for PointSurvey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DecodedPointSurvey::deserialize(deserializer)?.survey)
    }
}
