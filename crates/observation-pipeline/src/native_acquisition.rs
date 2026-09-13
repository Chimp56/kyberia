//! Canonical composition of one validated native session and its observations.
//!
//! This boundary accepts only the opaque [`NativeCaptureSession`] produced by
//! `process::run_and_normalize`. It retains the exact admitted mapping context
//! while deriving the batch and the durable domain session record from that
//! same object. Callers cannot provide an independent normalized capture,
//! mapping context or record to combine accidentally.

use crate::{BatchError, ReceivedObservationBatch, process::NativeCaptureSession};
use kyberia_capture_adapter::macos::{Completion, MappingContext, TerminalStatus};
use kyberia_domain::{
    ValidationError,
    capture_session::{
        CaptureSessionRecordV1, MAX_CAPTURE_SESSION_BYTES, MAX_SESSION_OBSERVATION_MAPPINGS,
        MAX_SESSION_SOURCE_MAPPINGS, MappingEvidenceV1, NativeUuid, ObservationMappingEvidenceV1,
        SourceMappingEvidenceV1,
    },
    evidence::Evidence,
    identity::{ContentHash, Text},
    observation::{ObservationEnvelope, PayloadRetention},
};
use sha2::{Digest, Sha256};
use std::fmt;

/// Errors at the native session-to-canonical acquisition boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeAcquisitionBatchError {
    Batch(BatchError),
    Domain(ValidationError),
    SessionMappingMismatch,
    CompletionMismatch,
    ManifestCanonicalBytes,
    RecordCanonicalBytes,
}

impl fmt::Display for NativeAcquisitionBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Batch(error) => error.fmt(formatter),
            Self::Domain(error) => write!(formatter, "native acquisition record: {error}"),
            Self::SessionMappingMismatch => {
                formatter.write_str("native session mapping is bound to another process session")
            }
            Self::CompletionMismatch => {
                formatter.write_str("native session completion differs from its normalized capture")
            }
            Self::ManifestCanonicalBytes => {
                formatter.write_str("native acquisition manifest cannot be canonicalized")
            }
            Self::RecordCanonicalBytes => {
                formatter.write_str("native acquisition record exceeds its canonical byte bound")
            }
        }
    }
}

impl std::error::Error for NativeAcquisitionBatchError {}

impl From<BatchError> for NativeAcquisitionBatchError {
    fn from(error: BatchError) -> Self {
        Self::Batch(error)
    }
}

impl From<ValidationError> for NativeAcquisitionBatchError {
    fn from(error: ValidationError) -> Self {
        Self::Domain(error)
    }
}

fn add_record_byte_floor(total: &mut usize, bytes: usize) -> bool {
    match total.checked_add(bytes) {
        Some(next) if next <= MAX_CAPTURE_SESSION_BYTES => {
            *total = next;
            true
        }
        _ => false,
    }
}

// This is deliberately below the actual fixed-field/punctuation overhead of
// a valid record. It makes the borrowed lower bound useful at the boundary
// while leaving the exact serializer as the authority for acceptance.
const MIN_RECORD_FIXED_BYTES: usize = 512;

fn preflight_text(value: &str) -> Result<(), NativeAcquisitionBatchError> {
    if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(NativeAcquisitionBatchError::Domain(
            ValidationError::InvalidText,
        ));
    }
    Ok(())
}

/// Reject obviously oversized mapping metadata before converting borrowed
/// context keys/evidence into owned domain rows. The floor counts raw UTF-8
/// bytes only; canonical JSON escaping and fixed-field overhead are covered
/// by the exact bounded serialization checks below.
fn preflight_record_size(
    context: &MappingContext,
    registry_version: &Text,
    session: &NativeCaptureSession,
    completion: &Completion,
) -> Result<(), NativeAcquisitionBatchError> {
    if context.sources.len() > MAX_SESSION_SOURCE_MAPPINGS
        || context.observations.len() > MAX_SESSION_OBSERVATION_MAPPINGS
    {
        return Err(NativeAcquisitionBatchError::Domain(
            ValidationError::ResourceLimit("session mappings"),
        ));
    }
    let mut floor = MIN_RECORD_FIXED_BYTES;
    let text_fields = [
        registry_version.as_str(),
        session.process_session(),
        session.clock_epoch(),
        completion.reason.as_str(),
        context.privacy.policy_version.as_str(),
    ];
    for text in text_fields {
        if !add_record_byte_floor(&mut floor, text.len()) {
            return Err(NativeAcquisitionBatchError::RecordCanonicalBytes);
        }
    }
    for source_key in context.sources.keys() {
        preflight_text(source_key)?;
        if !add_record_byte_floor(&mut floor, source_key.len()) {
            return Err(NativeAcquisitionBatchError::RecordCanonicalBytes);
        }
    }
    for (observation_key, observation) in &context.observations {
        preflight_text(observation_key)?;
        if !add_record_byte_floor(&mut floor, observation_key.len()) {
            return Err(NativeAcquisitionBatchError::RecordCanonicalBytes);
        }
        if let Evidence::Known(reference) = &observation.identity_evidence
            && !add_record_byte_floor(&mut floor, reference.media_type.as_str().len())
        {
            return Err(NativeAcquisitionBatchError::RecordCanonicalBytes);
        }
    }
    if let PayloadRetention::Retained {
        authorization_reference,
        ..
    } = &context.privacy.payload
        && !add_record_byte_floor(&mut floor, authorization_reference.as_str().len())
    {
        return Err(NativeAcquisitionBatchError::RecordCanonicalBytes);
    }
    Ok(())
}

/// A batch and durable provenance record derived from one opaque native
/// session. Both views are immutable, and the record is validated against the
/// batch's exact canonical manifest and envelope order before construction.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeAcquisitionBatch {
    batch: ReceivedObservationBatch,
    record: CaptureSessionRecordV1,
}

impl NativeAcquisitionBatch {
    /// Compose one native session into the canonical batch and session record.
    /// The registry version is explicit caller provenance; it is not inferred
    /// from a collector, executable path, or current process environment.
    pub fn from_session(
        session: &NativeCaptureSession,
        registry_version: Text,
    ) -> Result<Self, NativeAcquisitionBatchError> {
        let context = session.mapping();
        if context.expected_process_session != session.process_session() {
            return Err(NativeAcquisitionBatchError::SessionMappingMismatch);
        }

        let completion = &session.normalized().completion;
        if completion.status != session.terminal()
            || usize::from(completion.observation_count) != session.normalized().observations.len()
            || completion.partial
                != (completion.status != TerminalStatus::Ok
                    && !session.normalized().observations.is_empty())
        {
            return Err(NativeAcquisitionBatchError::CompletionMismatch);
        }
        preflight_record_size(context, &registry_version, session, completion)?;
        let source_mappings = context
            .sources
            .iter()
            .map(|(source_key, source)| {
                Ok(SourceMappingEvidenceV1::new(
                    Text::new(source_key.clone())?,
                    source.source_id,
                    source.sensor_id.clone(),
                    source.adapter_id.clone(),
                ))
            })
            .collect::<Result<Vec<_>, ValidationError>>()?;
        let observation_mappings = context
            .observations
            .iter()
            .map(|(observation_key, observation)| {
                Ok(ObservationMappingEvidenceV1::new(
                    Text::new(observation_key.clone())?,
                    observation.observation_id,
                    observation.transmitter_radio.clone(),
                    observation.transmitter_bss.clone(),
                    observation.identity_evidence.clone(),
                ))
            })
            .collect::<Result<Vec<_>, ValidationError>>()?;
        let mapping =
            MappingEvidenceV1::new(registry_version, source_mappings, observation_mappings)?;
        // A manifest hash is always a fixed-width ContentHash in the record's
        // canonical JSON. Probe with a zero hash before cloning the normalized
        // capture so a maximal mapping cannot create an oversized batch only
        // to reject it after composition.
        let preflight = CaptureSessionRecordV1::new(
            context.session_id,
            context.collector_id,
            context.clock_epoch,
            NativeUuid::new(session.process_session().to_owned())?,
            NativeUuid::new(session.clock_epoch().to_owned())?,
            ContentHash::from_sha256([0; 32]),
            mapping,
            context.privacy.clone(),
            completion.status.into(),
            completion.reason.clone(),
            completion.partial,
            completion.observation_count,
            session.exit_code(),
        )?;
        if preflight
            .canonical_bytes()
            .map_or(true, |bytes| bytes.len() > MAX_CAPTURE_SESSION_BYTES)
        {
            return Err(NativeAcquisitionBatchError::RecordCanonicalBytes);
        }

        let batch =
            ReceivedObservationBatch::from_normalized_capture(session.normalized().clone())?;
        let envelopes: Vec<ObservationEnvelope> = batch
            .observations()
            .iter()
            .map(|received| received.envelope().clone())
            .collect();
        let manifest_bytes = batch
            .manifest()
            .canonical_bytes()
            .map_err(|_| NativeAcquisitionBatchError::ManifestCanonicalBytes)?;
        let manifest_hash = ContentHash::from_sha256(Sha256::digest(manifest_bytes).into());
        let record = preflight.with_manifest_hash(manifest_hash);
        if record
            .canonical_bytes()
            .map_or(true, |bytes| bytes.len() > MAX_CAPTURE_SESSION_BYTES)
        {
            return Err(NativeAcquisitionBatchError::RecordCanonicalBytes);
        }
        record.validate_against_manifest(batch.manifest(), &envelopes)?;
        Ok(Self { batch, record })
    }

    pub const fn batch(&self) -> &ReceivedObservationBatch {
        &self.batch
    }

    pub const fn record(&self) -> &CaptureSessionRecordV1 {
        &self.record
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{
        NeverCancel,
        process::{CollectorCommand, ScanOptions, TrustedCollector, run_and_normalize},
    };
    use kyberia_capture_adapter::macos::{
        MappingContext, ObservationMapping, SourceMapping, decode,
    };
    use kyberia_domain::{
        capture::CaptureTerminalStatus,
        evidence::{ArtifactReference, Evidence, UnknownReason},
        identity::{
            AdapterId, BssId, ClockEpochId, CollectorId, ObservationId, RadioId, SensorId,
            SessionId, SourceId,
        },
        observation::{IdentifierPolicy, PayloadRetention, PrivacyState},
    };
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    const VALID: &[u8] = include_bytes!("../../../collectors/macos/fixtures/valid.ndjson");
    const EMPTY: &[u8] = include_bytes!("../../../collectors/macos/fixtures/empty.ndjson");
    const PARTIAL: &[u8] = include_bytes!("../../../collectors/macos/fixtures/partial.ndjson");
    static TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn unknown<T>() -> Evidence<T> {
        Evidence::Unknown(UnknownReason::SourceDidNotProvide)
    }

    fn retained_test_directory() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.trash/test-runs");
        fs::create_dir_all(&root).unwrap();
        let process = std::process::id();
        loop {
            let sequence = TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = root.join(format!("native-acquisition-batch-{process}-{sequence}"));
            match fs::create_dir(&directory) {
                Ok(()) => return directory,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create retained test directory: {error}"),
            }
        }
    }

    fn collector(fixture: &[u8]) -> (PathBuf, TrustedCollector) {
        collector_with_exit(fixture, if fixture == PARTIAL { 2 } else { 0 })
    }

    fn collector_with_exit(fixture: &[u8], exit_code: i32) -> (PathBuf, TrustedCollector) {
        let directory = retained_test_directory();
        let fixture_path = directory.join("capture.ndjson");
        fs::write(&fixture_path, fixture).unwrap();
        let script_path = directory.join("collector");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\n/bin/cat '{}'\nexit {}\n",
                fixture_path.display(),
                exit_code
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();
        let expected_build = decode(VALID).unwrap().collector_build();
        let trusted = TrustedCollector::new(&script_path, expected_build).unwrap();
        (directory, trusted)
    }

    fn privacy(redacted: bool) -> PrivacyState {
        PrivacyState {
            policy_version: Text::new("native-test-privacy/v1").unwrap(),
            identifiers: if redacted {
                IdentifierPolicy::Redacted
            } else {
                IdentifierPolicy::ExplicitResearchConsent
            },
            payload: PayloadRetention::Discarded,
        }
    }

    fn context(
        stream: &kyberia_capture_adapter::macos::DecodedStream,
        redacted: bool,
        known_transmitter: bool,
    ) -> MappingContext {
        let sources = stream
            .source_keys()
            .enumerate()
            .map(|(index, key)| {
                let mut id_bytes = [0; 16];
                id_bytes[0] = 40 + index as u8;
                (
                    key.to_owned(),
                    SourceMapping {
                        source_id: SourceId::from_bytes(id_bytes).unwrap(),
                        sensor_id: Evidence::Known(SensorId::from_bytes([41; 16]).unwrap()),
                        adapter_id: Evidence::Known(AdapterId::from_bytes([42; 16]).unwrap()),
                    },
                )
            })
            .collect();
        let observations = stream
            .observation_keys()
            .enumerate()
            .map(|(index, key)| {
                let mut id_bytes = [0; 16];
                id_bytes[0] = 80 + index as u8;
                let (transmitter_radio, transmitter_bss, identity_evidence) = if known_transmitter {
                    (
                        Evidence::Known(RadioId::from_bytes([43; 16]).unwrap()),
                        Evidence::Known(BssId::from_bytes([44; 16]).unwrap()),
                        Evidence::Known(ArtifactReference {
                            sha256: ContentHash::from_sha256([45; 32]),
                            media_type: Text::new("application/vnd.kyberia.identity-assignment")
                                .unwrap(),
                            byte_length: 1,
                        }),
                    )
                } else {
                    (unknown(), unknown(), unknown())
                };
                (
                    key.to_owned(),
                    ObservationMapping {
                        observation_id: ObservationId::from_bytes(id_bytes).unwrap(),
                        transmitter_radio,
                        transmitter_bss,
                        identity_evidence,
                    },
                )
            })
            .collect();
        MappingContext {
            expected_process_session: stream.process_session().to_owned(),
            session_id: SessionId::from_bytes([10; 16]).unwrap(),
            collector_id: CollectorId::from_bytes([11; 16]).unwrap(),
            clock_epoch: ClockEpochId::from_bytes([12; 16]).unwrap(),
            sources,
            observations,
            privacy: privacy(redacted),
        }
    }

    fn session(
        fixture: &[u8],
        redacted: bool,
        known_transmitter: bool,
    ) -> (PathBuf, NativeCaptureSession) {
        let (directory, collector) = collector(fixture);
        let command = CollectorCommand::Scan(ScanOptions::new(None, 4, 20, !redacted).unwrap());
        let session = run_and_normalize(
            &collector,
            command,
            |stream| Ok(context(stream, redacted, known_transmitter)),
            &NeverCancel,
        )
        .unwrap();
        (directory, session)
    }

    #[test]
    fn nonempty_session_preserves_receiver_and_transmitter_mapping() {
        let (_directory, session) = session(VALID, false, true);
        let composed = NativeAcquisitionBatch::from_session(
            &session,
            Text::new("registry/native-test-v1").unwrap(),
        )
        .unwrap();
        assert_eq!(composed.batch().observations().len(), 1);
        assert_eq!(
            composed.record().session_id(),
            SessionId::from_bytes([10; 16]).unwrap()
        );
        assert_eq!(
            composed.record().collector_id(),
            CollectorId::from_bytes([11; 16]).unwrap()
        );
        assert_eq!(
            composed.record().clock_epoch_id(),
            ClockEpochId::from_bytes([12; 16]).unwrap()
        );
        assert_eq!(
            composed.record().process_session_uuid().as_str(),
            session.process_session()
        );
        assert_eq!(
            composed.record().source_clock_uuid().as_str(),
            session.clock_epoch()
        );
        assert_eq!(composed.record().mapping().source_mappings().len(), 1);
        let source = &composed.record().mapping().source_mappings()[0];
        let mut expected_source_id = [0; 16];
        expected_source_id[0] = 40;
        assert_eq!(
            source.source_id(),
            SourceId::from_bytes(expected_source_id).unwrap()
        );
        assert!(matches!(source.sensor_id(), Evidence::Known(_)));
        assert!(matches!(source.adapter_id(), Evidence::Known(_)));
        let observation = &composed.record().mapping().observation_mappings()[0];
        assert!(matches!(
            observation.transmitter_radio(),
            Evidence::Known(_)
        ));
        assert!(matches!(observation.transmitter_bss(), Evidence::Known(_)));
        assert!(matches!(
            observation.identity_evidence(),
            Evidence::Known(_)
        ));
        assert_eq!(
            composed.record().mapping().registry_version().as_str(),
            "registry/native-test-v1"
        );
    }

    #[test]
    fn empty_terminal_preserves_source_mapping_and_privacy_policy() {
        let (_directory, session) = session(EMPTY, true, false);
        let composed = NativeAcquisitionBatch::from_session(
            &session,
            Text::new("registry/native-test-v1").unwrap(),
        )
        .unwrap();
        assert!(composed.batch().observations().is_empty());
        assert!(
            composed
                .record()
                .mapping()
                .observation_mappings()
                .is_empty()
        );
        assert_eq!(composed.record().mapping().source_mappings().len(), 1);
        assert_eq!(composed.record().privacy(), &session.mapping().privacy);
        assert_eq!(
            composed.batch().manifest().raw_source_disposition(),
            crate::RawSourceDisposition::NotRetained
        );
        assert_eq!(composed.record().observation_count(), 0);
    }

    #[test]
    fn empty_partial_terminal_preserves_source_completion_rule() {
        let mut records: Vec<serde_json::Value> = std::str::from_utf8(EMPTY)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let complete = records.last_mut().unwrap();
        complete["status"] = serde_json::json!("partial");
        complete["partial"] = serde_json::json!(false);
        let fixture = records
            .iter()
            .flat_map(|record| {
                let mut bytes = serde_json::to_vec(record).unwrap();
                bytes.push(b'\n');
                bytes
            })
            .collect::<Vec<_>>();
        let (_directory, collector) = collector_with_exit(&fixture, 2);
        let session = run_and_normalize(
            &collector,
            CollectorCommand::Scan(ScanOptions::new(None, 1, 20, false).unwrap()),
            |stream| Ok(context(stream, true, false)),
            &NeverCancel,
        )
        .unwrap();
        let composed = NativeAcquisitionBatch::from_session(
            &session,
            Text::new("registry/native-test-v1").unwrap(),
        )
        .unwrap();
        assert_eq!(composed.record().terminal(), CaptureTerminalStatus::Partial);
        assert!(!composed.record().partial());
        assert_eq!(composed.record().observation_count(), 0);
    }

    #[test]
    fn oversized_session_record_is_rejected_by_mapping_byte_preflight() {
        let (_directory, collector) = collector(EMPTY);
        let session = run_and_normalize(
            &collector,
            CollectorCommand::Scan(ScanOptions::new(None, 1, 20, false).unwrap()),
            |stream| {
                let mut context = context(stream, true, false);
                let remaining = kyberia_domain::capture_session::MAX_SESSION_SOURCE_MAPPINGS
                    .saturating_sub(context.sources.len());
                for index in 0..remaining {
                    let mut id = [0; 16];
                    id[..4].copy_from_slice(&u32::try_from(index).unwrap().to_le_bytes());
                    id[4] = 1;
                    context.sources.insert(
                        format!("extra-{index:04}-{}", "x".repeat(300)),
                        SourceMapping {
                            source_id: SourceId::from_bytes(id).unwrap(),
                            sensor_id: unknown(),
                            adapter_id: unknown(),
                        },
                    );
                }
                Ok(context)
            },
            &NeverCancel,
        )
        .unwrap();
        let result = NativeAcquisitionBatch::from_session(
            &session,
            Text::new("registry/native-test-v1").unwrap(),
        );
        assert!(matches!(
            result,
            Err(NativeAcquisitionBatchError::RecordCanonicalBytes)
        ));
    }

    #[test]
    fn canonical_overhead_rejects_after_mapping_byte_preflight() {
        let (_directory, collector) = collector(EMPTY);
        let session = run_and_normalize(
            &collector,
            CollectorCommand::Scan(ScanOptions::new(None, 1, 20, false).unwrap()),
            |stream| {
                let mut context = context(stream, true, false);
                let remaining = kyberia_domain::capture_session::MAX_SESSION_SOURCE_MAPPINGS
                    .saturating_sub(context.sources.len());
                for index in 0..remaining {
                    let mut id = [0; 16];
                    id[..4].copy_from_slice(&u32::try_from(index).unwrap().to_le_bytes());
                    id[4] = 1;
                    context.sources.insert(
                        format!("extra-{index:04}-{}", "x".repeat(220)),
                        SourceMapping {
                            source_id: SourceId::from_bytes(id).unwrap(),
                            sensor_id: unknown(),
                            adapter_id: unknown(),
                        },
                    );
                }
                Ok(context)
            },
            &NeverCancel,
        )
        .unwrap();
        let result = NativeAcquisitionBatch::from_session(
            &session,
            Text::new("registry/native-test-v1").unwrap(),
        );
        assert!(matches!(
            result,
            Err(NativeAcquisitionBatchError::RecordCanonicalBytes)
        ));
    }

    #[test]
    fn mapping_byte_floor_rejects_boundary_and_checked_add_overflow() {
        let mut exact = MAX_CAPTURE_SESSION_BYTES - 1;
        assert!(add_record_byte_floor(&mut exact, 1));
        assert_eq!(exact, MAX_CAPTURE_SESSION_BYTES);
        assert!(!add_record_byte_floor(&mut exact, 1));

        let mut overflow = usize::MAX;
        assert!(!add_record_byte_floor(&mut overflow, 1));
        assert_eq!(overflow, usize::MAX);
    }

    #[test]
    fn partial_session_retains_terminal_and_partial_evidence() {
        let (_directory, session) = session(PARTIAL, true, false);
        let composed = NativeAcquisitionBatch::from_session(
            &session,
            Text::new("registry/native-test-v1").unwrap(),
        )
        .unwrap();
        assert_eq!(composed.record().terminal(), CaptureTerminalStatus::Partial);
        assert!(composed.record().partial());
        assert_eq!(composed.record().exit_code(), 2);
        assert_eq!(composed.record().observation_count(), 1);
    }

    #[test]
    fn non_ok_observations_preserve_source_partial_rule() {
        let cases = [
            ("partial", "partial", 2),
            ("permission_required", "permission required", 77),
            ("unsupported", "unsupported", 69),
            ("unavailable", "unavailable", 69),
            ("timeout", "timed out", 124),
            ("cancelled", "cancelled", 130),
            ("error", "collector error", 70),
        ];
        for (status, reason, exit_code) in cases {
            let mut records: Vec<serde_json::Value> = std::str::from_utf8(VALID)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            let complete = records.last_mut().unwrap();
            complete["status"] = serde_json::json!(status);
            complete["reason"] = serde_json::json!(reason);
            complete["partial"] = serde_json::json!(true);
            if status == "error" {
                complete["native_error_domain"] = serde_json::json!("native-test");
                complete["native_error_code"] = serde_json::json!(1);
            }
            let fixture = records
                .iter()
                .flat_map(|record| {
                    let mut bytes = serde_json::to_vec(record).unwrap();
                    bytes.push(b'\n');
                    bytes
                })
                .collect::<Vec<_>>();
            let (_directory, collector) = collector_with_exit(&fixture, exit_code);
            let session = run_and_normalize(
                &collector,
                CollectorCommand::Scan(ScanOptions::new(None, 4, 20, true).unwrap()),
                |stream| Ok(context(stream, false, false)),
                &NeverCancel,
            )
            .unwrap();
            let composed = NativeAcquisitionBatch::from_session(
                &session,
                Text::new("registry/native-test-v1").unwrap(),
            )
            .unwrap();
            assert!(composed.record().partial());
            assert_eq!(composed.record().observation_count(), 1);
        }
    }

    #[test]
    fn mismatched_process_mapping_is_rejected_before_composition() {
        let (_directory, collector) = collector(VALID);
        let result = run_and_normalize(
            &collector,
            CollectorCommand::Scan(ScanOptions::new(None, 4, 20, true).unwrap()),
            |stream| {
                let mut context = context(stream, false, false);
                context.expected_process_session = "00000000-0000-4000-8000-000000000099".into();
                Ok(context)
            },
            &NeverCancel,
        );
        assert!(matches!(
            result,
            Err(crate::process::NativeCaptureSessionError::AdapterNormalize(
                _
            ))
        ));
    }

    #[test]
    fn mismatched_transmitter_evidence_is_rejected_before_composition() {
        let (_directory, collector) = collector(VALID);
        let result = run_and_normalize(
            &collector,
            CollectorCommand::Scan(ScanOptions::new(None, 4, 20, true).unwrap()),
            |stream| {
                let mut context = context(stream, false, true);
                context
                    .observations
                    .values_mut()
                    .next()
                    .unwrap()
                    .identity_evidence = unknown();
                Ok(context)
            },
            &NeverCancel,
        );
        assert!(matches!(
            result,
            Err(crate::process::NativeCaptureSessionError::AdapterNormalize(
                _
            ))
        ));
    }

    #[test]
    fn retained_payload_policy_is_rejected_before_native_composition() {
        let (_directory, collector) = collector(VALID);
        let result = run_and_normalize(
            &collector,
            CollectorCommand::Scan(ScanOptions::new(None, 4, 20, true).unwrap()),
            |stream| {
                let mut context = context(stream, false, false);
                context.privacy.payload = PayloadRetention::Retained {
                    authorization_reference: Text::new("native-test-consent/v1").unwrap(),
                    retention_deadline: kyberia_domain::time::UtcTimestamp(2_000_000),
                };
                Ok(context)
            },
            &NeverCancel,
        );
        assert!(matches!(
            result,
            Err(crate::process::NativeCaptureSessionError::AdapterNormalize(
                _
            ))
        ));
    }

    #[test]
    fn source_and_observation_mapping_keys_are_canonicalized() {
        let (_directory, session) = session(VALID, false, false);
        let composed = NativeAcquisitionBatch::from_session(
            &session,
            Text::new("registry/native-test-v1").unwrap(),
        )
        .unwrap();
        let source_key = composed.record().mapping().source_mappings()[0]
            .source_key()
            .as_str();
        let observation_key = composed.record().mapping().observation_mappings()[0]
            .observation_key()
            .as_str();
        assert_eq!(
            source_key,
            session.mapping().sources.first_key_value().unwrap().0
        );
        assert_eq!(
            observation_key,
            session.mapping().observations.first_key_value().unwrap().0
        );
    }
}
