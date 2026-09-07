//! Typed, version-checked wire decoding; no JSON value tree or foreign schema.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationInputVersion {
    #[serde(rename = "1")]
    V1,
    #[serde(rename = "2")]
    V2,
}

/// Decoder-produced provenance. Store original bytes/hash and this receipt
/// together; it does not authenticate the upstream source or rewrite artifacts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservationDecodeReceipt {
    pub input_schema_version: ObservationInputVersion,
    pub output_schema_version: ObservationSchemaVersion,
    pub decoder_version: &'static str,
    pub migrated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedObservation {
    pub envelope: ObservationEnvelope,
    pub receipt: ObservationDecodeReceipt,
}

// Only the changed scalar is a wire union. The enclosing explicit schema tag
// determines its admissible shape; neither format silently falls back to another.
#[derive(Deserialize)]
#[serde(untagged)]
enum VersionValue {
    Legacy(Text),
    Current(Evidence<Text>),
}

#[derive(Deserialize)]
struct SourceWire {
    source_id: SourceId,
    collector_id: CollectorId,
    sensor_id: Evidence<SensorId>,
    adapter_id: Evidence<AdapterId>,
    kind: SourceKind,
    source_name: Text,
    source_version: VersionValue,
    source_schema_version: Text,
    adapter_name: Text,
    adapter_version: Text,
    parser_version: Text,
    driver_version: Evidence<Text>,
    os_version: Evidence<Text>,
}

#[derive(Deserialize)]
struct EnvelopeWire {
    schema_version: ObservationInputVersion,
    id: ObservationId,
    session_id: SessionId,
    source: SourceWire,
    time: CaptureTime,
    pose: Evidence<PoseReference>,
    channel: Evidence<ChannelContext>,
    dwell: Evidence<DwellContext>,
    privacy: PrivacyState,
    quality: Vec<QualityFlag>,
    raw_source: Evidence<ArtifactReference>,
    payload: ObservationPayload,
}

impl<'de> Deserialize<'de> for DecodedObservation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = EnvelopeWire::deserialize(deserializer)?;
        let source_version = match (wire.schema_version, wire.source.source_version) {
            (ObservationInputVersion::V1, VersionValue::Legacy(version)) => {
                Evidence::Known(version)
            }
            (ObservationInputVersion::V2, VersionValue::Current(version)) => version,
            _ => {
                return Err(serde::de::Error::custom(
                    "source version shape disagrees with observation schema",
                ));
            }
        };
        let envelope = ObservationEnvelope::new(EnvelopeData {
            schema_version: ObservationSchemaVersion::V2,
            id: wire.id,
            session_id: wire.session_id,
            source: SourceDescriptor {
                source_id: wire.source.source_id,
                collector_id: wire.source.collector_id,
                sensor_id: wire.source.sensor_id,
                adapter_id: wire.source.adapter_id,
                kind: wire.source.kind,
                source_name: wire.source.source_name,
                source_version,
                source_schema_version: wire.source.source_schema_version,
                adapter_name: wire.source.adapter_name,
                adapter_version: wire.source.adapter_version,
                parser_version: wire.source.parser_version,
                driver_version: wire.source.driver_version,
                os_version: wire.source.os_version,
            },
            time: wire.time,
            pose: wire.pose,
            channel: wire.channel,
            dwell: wire.dwell,
            privacy: wire.privacy,
            quality: wire.quality,
            raw_source: wire.raw_source,
            payload: wire.payload,
        })
        .map_err(serde::de::Error::custom)?;
        Ok(Self {
            envelope,
            receipt: ObservationDecodeReceipt {
                input_schema_version: wire.schema_version,
                output_schema_version: ObservationSchemaVersion::V2,
                decoder_version: "kyberia-observation/2.0.0",
                migrated: wire.schema_version == ObservationInputVersion::V1,
            },
        })
    }
}

impl<'de> Deserialize<'de> for ObservationEnvelope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(DecodedObservation::deserialize(deserializer)?.envelope)
    }
}
