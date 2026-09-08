//! Immutable typed observation chunks.
//!
//! Immutable typed observation chunks backed by the official Apache Arrow Rust
//! Parquet implementation. The domain remains independent of storage and
//! analytical-library objects; all Arrow/Parquet values are confined to the
//! inward adapter in `parquet_codec`.

use crate::bundle::{atomic_projection, load_manifest};
use crate::manifest::{MAX_ARTIFACT_BYTES, validate_hash, validate_text};
use crate::{
    ArtifactEntry, ArtifactKind, Bundle, OpenMode, Result, StoreError, content_hash, parquet_codec,
    sqlite_guard,
};
use kyberia_domain::identity::{ObservationId, SessionId, SourceId};
use kyberia_domain::observation::ObservationEnvelope;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The current canonical ObservationEnvelope schema.
pub const OBSERVATION_SCHEMA_VERSION: u32 = 2;
/// Version of the bounded native fallback framing codec.
pub const OBSERVATION_CHUNK_CODEC_VERSION: u32 = parquet_codec::CODEC_VERSION;
/// A separate wire-format version allows future framing changes without
/// silently interpreting an old artifact with new rules.
pub const OBSERVATION_CHUNK_FORMAT_VERSION: u32 = parquet_codec::FORMAT_VERSION;
/// Canonical analytical chunk media type.
pub const PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE: &str = parquet_codec::MEDIA_TYPE;
pub const PARQUET_SCHEMA_FINGERPRINT: &str = parquet_codec::SCHEMA_FINGERPRINT;
/// Reserved media type for an explicitly separate live spool. It is not
/// accepted by the committed analytical chunk reader.
pub const NATIVE_OBSERVATION_CHUNK_MEDIA_TYPE: &str =
    "application/vnd.kyberia.observation-chunk; format=native; version=1";
/// Maximum number of envelopes in one immutable chunk.
pub const MAX_OBSERVATION_CHUNK_ROWS: u64 = 65_536;
/// Maximum serialized size for one envelope in the bounded reader.
pub const MAX_OBSERVATION_ROW_BYTES: u64 = 4 * 1024 * 1024;
/// Chunk artifacts share the bundle's 64 MiB bounded artifact envelope.
pub const MAX_OBSERVATION_CHUNK_BYTES: u64 = MAX_ARTIFACT_BYTES;
/// Maximum number of committed chunk rows enumerated by one metadata read.
pub const MAX_OBSERVATION_CHUNKS: u64 = 10_000;
/// Maximum rows materialized by `read_observations` in one call.
pub const MAX_OBSERVATIONS_PER_READ: u64 = 2_000_000;

/// Provenance and publication metadata supplied by the owning use case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservationChunkProvenance {
    /// A stable source/provenance record, never a filesystem path.
    provenance_id: String,
}

impl<'de> Deserialize<'de> for ObservationChunkProvenance {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            provenance_id: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.provenance_id).map_err(serde::de::Error::custom)
    }
}

impl ObservationChunkProvenance {
    pub fn new(provenance_id: impl Into<String>) -> Result<Self> {
        let provenance_id = provenance_id.into();
        validate_text(&provenance_id, 1024)?;
        Ok(Self { provenance_id })
    }
    pub fn provenance_id(&self) -> &str {
        &self.provenance_id
    }
}

impl From<ObservationChunkProvenance> for String {
    fn from(provenance: ObservationChunkProvenance) -> Self {
        provenance.provenance_id
    }
}

/// Committed metadata for one immutable observation chunk. Every field is
/// derived from the finalized bytes and authoritative SQLite index, then
/// checked again when read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservationChunkDescriptor {
    hash: String,
    bytes: u64,
    media_type: String,
    schema_version: u32,
    codec_version: u32,
    row_count: u64,
    first_observation_id: ObservationId,
    last_observation_id: ObservationId,
    /// UTC bounds are absent when no envelope in the chunk has a wall-clock
    /// reading. Absence is never represented as numeric zero.
    known_utc_count: u64,
    first_utc_ns: Option<i64>,
    last_utc_ns: Option<i64>,
    first_source_id: SourceId,
    last_source_id: SourceId,
    first_session_id: SessionId,
    last_session_id: SessionId,
    provenance_id: String,
    revision: u64,
}

impl<'de> Deserialize<'de> for ObservationChunkDescriptor {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            hash: String,
            bytes: u64,
            media_type: String,
            schema_version: u32,
            codec_version: u32,
            row_count: u64,
            first_observation_id: ObservationId,
            last_observation_id: ObservationId,
            known_utc_count: u64,
            first_utc_ns: Option<i64>,
            last_utc_ns: Option<i64>,
            first_source_id: SourceId,
            last_source_id: SourceId,
            first_session_id: SessionId,
            last_session_id: SessionId,
            provenance_id: String,
            revision: u64,
        }
        let wire = Wire::deserialize(deserializer)?;
        let descriptor = Self {
            hash: wire.hash,
            bytes: wire.bytes,
            media_type: wire.media_type,
            schema_version: wire.schema_version,
            codec_version: wire.codec_version,
            row_count: wire.row_count,
            first_observation_id: wire.first_observation_id,
            last_observation_id: wire.last_observation_id,
            known_utc_count: wire.known_utc_count,
            first_utc_ns: wire.first_utc_ns,
            last_utc_ns: wire.last_utc_ns,
            first_source_id: wire.first_source_id,
            last_source_id: wire.last_source_id,
            first_session_id: wire.first_session_id,
            last_session_id: wire.last_session_id,
            provenance_id: wire.provenance_id,
            revision: wire.revision,
        };
        validate_descriptor(&descriptor).map_err(serde::de::Error::custom)?;
        Ok(descriptor)
    }
}

impl ObservationChunkDescriptor {
    pub fn hash(&self) -> &str {
        &self.hash
    }
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub const fn codec_version(&self) -> u32 {
        self.codec_version
    }
    pub const fn row_count(&self) -> u64 {
        self.row_count
    }
    pub const fn first_observation_id(&self) -> ObservationId {
        self.first_observation_id
    }
    pub const fn last_observation_id(&self) -> ObservationId {
        self.last_observation_id
    }
    pub const fn known_utc_count(&self) -> u64 {
        self.known_utc_count
    }
    pub const fn first_utc_ns(&self) -> Option<i64> {
        self.first_utc_ns
    }
    pub const fn last_utc_ns(&self) -> Option<i64> {
        self.last_utc_ns
    }
    pub const fn first_source_id(&self) -> SourceId {
        self.first_source_id
    }
    pub const fn last_source_id(&self) -> SourceId {
        self.last_source_id
    }
    pub const fn first_session_id(&self) -> SessionId {
        self.first_session_id
    }
    pub const fn last_session_id(&self) -> SessionId {
        self.last_session_id
    }
    pub fn provenance_id(&self) -> &str {
        &self.provenance_id
    }
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Revalidate metadata obtained from an untrusted boundary before use.
    pub fn validate(&self) -> Result<()> {
        validate_descriptor(self)
    }
}

#[derive(Clone, Debug)]
struct PreparedChunk {
    bytes: Vec<u8>,
    observations: Vec<ObservationEnvelope>,
    descriptor: ObservationChunkDescriptor,
}

/// A cancellation callback is checked before expensive work, before durable
/// publication and before the metadata transaction. If cancellation wins
/// after artifact publication, the complete unreferenced artifact is retained
/// for explicit future garbage collection; it is never made visible by reads.
pub trait Cancellation {
    fn is_cancelled(&self) -> bool;
}

impl<F> Cancellation for F
where
    F: Fn() -> bool,
{
    fn is_cancelled(&self) -> bool {
        self()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Narrow inward port implemented by the project-store adapter. Domain and
/// application code depend on this contract rather than on SQLite, framing,
/// or external analytical-library objects.
pub trait ObservationChunkStore {
    fn publish_observation_chunk(
        &mut self,
        observations: &[ObservationEnvelope],
        provenance: ObservationChunkProvenance,
        utc_ms: i64,
    ) -> Result<ObservationChunkDescriptor>;
    fn list_observation_chunks(&self) -> Result<Vec<ObservationChunkDescriptor>>;
    fn read_observation_chunk(&self, hash: &str) -> Result<Vec<ObservationEnvelope>>;
}

fn cancelled() -> StoreError {
    StoreError::Cancelled
}

fn check_cancel<C: Cancellation>(cancel: &C) -> Result<()> {
    if cancel.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}

fn canonical_observations<C: Cancellation>(
    observations: &[ObservationEnvelope],
    provenance: &ObservationChunkProvenance,
    cancel: &C,
) -> Result<PreparedChunk> {
    check_cancel(cancel)?;
    if observations.is_empty() {
        return Err(StoreError::Invalid(
            "an observation chunk cannot be empty".into(),
        ));
    }
    if observations.len() as u64 > MAX_OBSERVATION_CHUNK_ROWS {
        return Err(StoreError::Invalid(
            "observation chunk exceeds row limit".into(),
        ));
    }
    provenance.clone().validate()?;

    let mut ordered = observations.to_vec();
    ordered.sort_by_key(|observation| observation.data().id);
    let mut ids = BTreeSet::new();
    for observation in &ordered {
        check_cancel(cancel)?;
        if !ids.insert(observation.data().id) {
            return Err(StoreError::Invalid(
                "duplicate observation identity within chunk".into(),
            ));
        }
    }

    check_cancel(cancel)?;
    let bytes = parquet_codec::encode(&ordered)?;

    let (first, last) = (
        ordered.first().expect("nonempty checked above"),
        ordered.last().expect("nonempty checked above"),
    );
    let utc_values = ordered
        .iter()
        .filter_map(|observation| match &observation.data().time.wall {
            kyberia_domain::evidence::Evidence::Known(reading) => Some(reading.time.0),
            kyberia_domain::evidence::Evidence::Unknown(_) => None,
        });
    let (known_utc_count, first_utc_ns, last_utc_ns) =
        utc_values.fold((0_u64, None, None), |(count, minimum, maximum), value| {
            (
                count + 1,
                Some(minimum.map_or(value, |minimum: i64| minimum.min(value))),
                Some(maximum.map_or(value, |maximum: i64| maximum.max(value))),
            )
        });
    let (first_source_id, last_source_id) = ordered
        .iter()
        .map(|observation| observation.data().source.source_id)
        .fold(
            (first.data().source.source_id, first.data().source.source_id),
            |(minimum, maximum), id| (minimum.min(id), maximum.max(id)),
        );
    let (first_session_id, last_session_id) = ordered
        .iter()
        .map(|observation| observation.data().session_id)
        .fold(
            (first.data().session_id, first.data().session_id),
            |(minimum, maximum), id| (minimum.min(id), maximum.max(id)),
        );
    Ok(PreparedChunk {
        descriptor: ObservationChunkDescriptor {
            hash: content_hash(&bytes),
            bytes: bytes.len() as u64,
            media_type: PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE.into(),
            schema_version: OBSERVATION_SCHEMA_VERSION,
            codec_version: OBSERVATION_CHUNK_CODEC_VERSION,
            row_count: ordered.len() as u64,
            first_observation_id: first.data().id,
            last_observation_id: last.data().id,
            known_utc_count,
            first_utc_ns,
            last_utc_ns,
            first_source_id,
            last_source_id,
            first_session_id,
            last_session_id,
            provenance_id: provenance.provenance_id.clone(),
            revision: 0,
        },
        bytes,
        observations: ordered,
    })
}

impl ObservationChunkProvenance {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.provenance_id, 1024)
    }
}

fn decode_chunk(bytes: &[u8]) -> Result<Vec<ObservationEnvelope>> {
    parquet_codec::decode(bytes)
}

fn descriptor_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ObservationChunkDescriptor> {
    let hash: String = row.get(0)?;
    let bytes: i64 = row.get(1)?;
    let media_type: String = row.get(2)?;
    let schema_version: i64 = row.get(3)?;
    let codec_version: i64 = row.get(4)?;
    let row_count: i64 = row.get(5)?;
    let first_observation_id: String = row.get(6)?;
    let last_observation_id: String = row.get(7)?;
    let known_utc_count: i64 = row.get(8)?;
    let first_utc_ns: Option<i64> = row.get(9)?;
    let last_utc_ns: Option<i64> = row.get(10)?;
    let first_source_id: String = row.get(11)?;
    let last_source_id: String = row.get(12)?;
    let first_session_id: String = row.get(13)?;
    let last_session_id: String = row.get(14)?;
    let provenance_id: String = row.get(15)?;
    let revision: i64 = row.get(16)?;
    let parse_id =
        |value: String| ObservationId::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery);
    let parse_source =
        |value: String| SourceId::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery);
    let parse_session =
        |value: String| SessionId::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery);
    Ok(ObservationChunkDescriptor {
        hash,
        bytes: u64::try_from(bytes).map_err(|_| rusqlite::Error::InvalidQuery)?,
        media_type,
        schema_version: u32::try_from(schema_version).map_err(|_| rusqlite::Error::InvalidQuery)?,
        codec_version: u32::try_from(codec_version).map_err(|_| rusqlite::Error::InvalidQuery)?,
        row_count: u64::try_from(row_count).map_err(|_| rusqlite::Error::InvalidQuery)?,
        first_observation_id: parse_id(first_observation_id)?,
        last_observation_id: parse_id(last_observation_id)?,
        known_utc_count: u64::try_from(known_utc_count)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        first_utc_ns,
        last_utc_ns,
        first_source_id: parse_source(first_source_id)?,
        last_source_id: parse_source(last_source_id)?,
        first_session_id: parse_session(first_session_id)?,
        last_session_id: parse_session(last_session_id)?,
        provenance_id,
        revision: u64::try_from(revision).map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

fn descriptor_query(
    transaction: &rusqlite::Transaction<'_>,
    hash: Option<&str>,
) -> Result<Vec<ObservationChunkDescriptor>> {
    let sql = "SELECT chunk_hash,bytes,media_type,schema_version,codec_version,row_count,first_observation_id,last_observation_id,known_utc_count,first_utc_ns,last_utc_ns,first_source_id,last_source_id,first_session_id,last_session_id,provenance_id,revision FROM observation_chunks";
    if let Some(hash) = hash {
        Ok(transaction
            .query_row(
                &format!("{sql} WHERE chunk_hash=?1"),
                [hash],
                descriptor_from_row,
            )
            .optional()?
            .into_iter()
            .collect())
    } else {
        let mut statement = transaction.prepare(&format!(
            "{sql} ORDER BY revision,chunk_hash LIMIT {}",
            MAX_OBSERVATION_CHUNKS + 1
        ))?;
        let mut rows = statement.query([])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            result.push(descriptor_from_row(row)?);
        }
        Ok(result)
    }
}

fn validate_descriptor(descriptor: &ObservationChunkDescriptor) -> Result<()> {
    validate_hash(&descriptor.hash)?;
    if descriptor.bytes == 0 || descriptor.bytes > MAX_OBSERVATION_CHUNK_BYTES {
        return Err(StoreError::Corrupt(
            "invalid observation chunk byte length".into(),
        ));
    }
    if descriptor.media_type != PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE {
        return Err(StoreError::Corrupt(
            "unsupported observation chunk media type".into(),
        ));
    }
    if descriptor.schema_version != OBSERVATION_SCHEMA_VERSION
        || descriptor.codec_version != OBSERVATION_CHUNK_CODEC_VERSION
    {
        return Err(StoreError::UnsupportedChunkVersion(
            descriptor.schema_version.max(descriptor.codec_version),
        ));
    }
    if descriptor.row_count == 0 || descriptor.row_count > MAX_OBSERVATION_CHUNK_ROWS {
        return Err(StoreError::Corrupt(
            "invalid observation chunk row count".into(),
        ));
    }
    if descriptor.revision == 0 {
        return Err(StoreError::Corrupt(
            "observation chunk revision is zero".into(),
        ));
    }
    if descriptor.first_observation_id > descriptor.last_observation_id
        || descriptor.first_source_id > descriptor.last_source_id
        || descriptor.first_session_id > descriptor.last_session_id
        || descriptor.known_utc_count > descriptor.row_count
        || (descriptor.known_utc_count == 0
            && (descriptor.first_utc_ns.is_some() || descriptor.last_utc_ns.is_some()))
        || (descriptor.known_utc_count > 0
            && (descriptor.first_utc_ns.is_none()
                || descriptor.last_utc_ns.is_none()
                || descriptor.first_utc_ns > descriptor.last_utc_ns))
    {
        return Err(StoreError::Corrupt(
            "invalid observation chunk range".into(),
        ));
    }
    validate_text(&descriptor.provenance_id, 1024)?;
    Ok(())
}

fn chunk_artifact_entry(descriptor: &ObservationChunkDescriptor) -> ArtifactEntry {
    ArtifactEntry {
        kind: ArtifactKind::NormalizedObservations,
        bytes: descriptor.bytes,
        media_type: descriptor.media_type.clone(),
        provenance_id: descriptor.provenance_id.clone(),
    }
}

fn verify_members(
    transaction: &rusqlite::Transaction<'_>,
    descriptor: &ObservationChunkDescriptor,
    observations: &[ObservationEnvelope],
) -> Result<()> {
    let mut statement = transaction.prepare(
        "SELECT observation_id,source_id,session_id,ordinal FROM observation_chunk_members WHERE chunk_hash=?1 ORDER BY ordinal",
    )?;
    let mut rows = statement.query([&descriptor.hash])?;
    let mut count = 0_u64;
    let mut ids = BTreeSet::new();
    while let Some(row) = rows.next()? {
        let observation_id: String = row.get(0)?;
        let source_id: String = row.get(1)?;
        let session_id: String = row.get(2)?;
        let ordinal: i64 = row.get(3)?;
        if u64::try_from(ordinal).ok() != Some(count)
            || !ids.insert(observation_id.clone())
            || ObservationId::try_from(observation_id.clone()).is_err()
            || SourceId::try_from(source_id.clone()).is_err()
            || SessionId::try_from(session_id.clone()).is_err()
        {
            return Err(StoreError::Corrupt(
                "malformed observation chunk index".into(),
            ));
        }
        let expected = observations
            .get(count as usize)
            .ok_or_else(|| StoreError::Corrupt("observation chunk index has extra rows".into()))?;
        if String::from(expected.data().id) != observation_id
            || String::from(expected.data().source.source_id) != source_id
            || String::from(expected.data().session_id) != session_id
        {
            return Err(StoreError::Corrupt(
                "observation chunk index does not match bytes".into(),
            ));
        }
        count += 1;
    }
    if count != descriptor.row_count || count != observations.len() as u64 {
        return Err(StoreError::Corrupt(
            "observation chunk index cardinality mismatch".into(),
        ));
    }
    Ok(())
}

fn verify_descriptor(
    bundle: &Bundle,
    descriptor: &ObservationChunkDescriptor,
) -> Result<Vec<ObservationEnvelope>> {
    validate_descriptor(descriptor)?;
    let manifest = bundle.manifest()?;
    let entry = manifest
        .artifacts
        .get(&descriptor.hash)
        .ok_or_else(|| StoreError::Corrupt("observation chunk is absent from manifest".into()))?;
    if entry != &chunk_artifact_entry(descriptor) {
        return Err(StoreError::Corrupt(
            "observation chunk manifest entry mismatch".into(),
        ));
    }
    let bytes = bundle.read_registered_artifact(&descriptor.hash, entry)?;
    if bytes.len() as u64 != descriptor.bytes || content_hash(&bytes) != descriptor.hash {
        return Err(StoreError::Corrupt(
            "observation chunk checksum/length mismatch".into(),
        ));
    }
    let observations = decode_chunk(&bytes)?;
    if observations.len() as u64 != descriptor.row_count
        || observations
            .first()
            .map(|observation| observation.data().id)
            != Some(descriptor.first_observation_id)
        || observations.last().map(|observation| observation.data().id)
            != Some(descriptor.last_observation_id)
    {
        return Err(StoreError::Corrupt(
            "observation chunk metadata does not match bytes".into(),
        ));
    }
    let mut known_utc =
        observations
            .iter()
            .filter_map(|observation| match &observation.data().time.wall {
                kyberia_domain::evidence::Evidence::Known(reading) => Some(reading.time.0),
                kyberia_domain::evidence::Evidence::Unknown(_) => None,
            });
    let (expected_known_utc_count, expected_utc) = known_utc
        .next()
        .map(|first| {
            observations
                .iter()
                .fold(
                    (first, first),
                    |(minimum, maximum), observation| match &observation.data().time.wall {
                        kyberia_domain::evidence::Evidence::Known(reading) => {
                            (minimum.min(reading.time.0), maximum.max(reading.time.0))
                        }
                        kyberia_domain::evidence::Evidence::Unknown(_) => (minimum, maximum),
                    },
                )
        })
        .map_or((0, None), |bounds| {
            let known_count = observations
                .iter()
                .filter(|observation| {
                    matches!(
                        &observation.data().time.wall,
                        kyberia_domain::evidence::Evidence::Known(_)
                    )
                })
                .count() as u64;
            (known_count, Some(bounds))
        });
    let (expected_first_source, expected_last_source) = observations
        .iter()
        .map(|observation| observation.data().source.source_id)
        .fold(
            (
                observations[0].data().source.source_id,
                observations[0].data().source.source_id,
            ),
            |(minimum, maximum), id| (minimum.min(id), maximum.max(id)),
        );
    let (expected_first_session, expected_last_session) = observations
        .iter()
        .map(|observation| observation.data().session_id)
        .fold(
            (
                observations[0].data().session_id,
                observations[0].data().session_id,
            ),
            |(minimum, maximum), id| (minimum.min(id), maximum.max(id)),
        );
    if descriptor.known_utc_count != expected_known_utc_count
        || descriptor.first_utc_ns != expected_utc.map(|bounds| bounds.0)
        || descriptor.last_utc_ns != expected_utc.map(|bounds| bounds.1)
        || descriptor.first_source_id != expected_first_source
        || descriptor.last_source_id != expected_last_source
        || descriptor.first_session_id != expected_first_session
        || descriptor.last_session_id != expected_last_session
    {
        return Err(StoreError::Corrupt(
            "observation chunk ranges do not match bytes".into(),
        ));
    }
    let transaction =
        rusqlite::Transaction::new_unchecked(&bundle.connection, TransactionBehavior::Deferred)?;
    verify_members(&transaction, descriptor, &observations)?;
    transaction.commit()?;
    Ok(observations)
}

fn inventory(bundle: &Bundle) -> Result<Vec<ObservationChunkDescriptor>> {
    if !sqlite_guard::has_observation_chunk_schema(&bundle.connection)? {
        return Ok(Vec::new());
    }
    bundle.start_operation()?;
    let transaction =
        rusqlite::Transaction::new_unchecked(&bundle.connection, TransactionBehavior::Deferred)?;
    let manifest = load_manifest(&transaction)?;
    let descriptors = descriptor_query(&transaction, None)?;
    if descriptors.len() as u64 > MAX_OBSERVATION_CHUNKS {
        return Err(StoreError::Corrupt(
            "observation chunk inventory exceeds resource limit".into(),
        ));
    }
    let normalized_artifacts = manifest
        .artifacts
        .iter()
        .filter(|(_, entry)| matches!(entry.kind, ArtifactKind::NormalizedObservations))
        .map(|(hash, _)| hash.clone())
        .collect::<BTreeSet<_>>();
    let descriptor_hashes = descriptors
        .iter()
        .map(|descriptor| descriptor.hash.clone())
        .collect::<BTreeSet<_>>();
    if normalized_artifacts != descriptor_hashes {
        return Err(StoreError::Corrupt(
            "observation manifest/index bijection failed".into(),
        ));
    }
    let mut member_statement = transaction.prepare(
        "SELECT chunk_hash FROM observation_chunk_members GROUP BY chunk_hash ORDER BY chunk_hash",
    )?;
    let mut member_rows = member_statement.query([])?;
    let mut member_hashes = BTreeSet::new();
    while let Some(row) = member_rows.next()? {
        let hash: String = row.get(0)?;
        if !member_hashes.insert(hash.clone()) {
            return Err(StoreError::Corrupt(
                "duplicate observation member chunk index".into(),
            ));
        }
        if !descriptor_hashes.contains(&hash) {
            return Err(StoreError::Corrupt(
                "observation member index references an uncommitted chunk".into(),
            ));
        }
    }
    if member_hashes != descriptor_hashes {
        return Err(StoreError::Corrupt(
            "observation member/chunk bijection failed".into(),
        ));
    }
    drop(member_rows);
    drop(member_statement);
    transaction.commit()?;
    for descriptor in &descriptors {
        verify_descriptor(bundle, descriptor)?;
    }
    Ok(descriptors)
}

impl Bundle {
    /// Publish one bounded immutable chunk. Rows are canonically ordered by
    /// observation identity; bytes are finalized and synced before SQLite
    /// commits the manifest/index/member transaction.
    pub fn publish_observation_chunk(
        &mut self,
        observations: &[ObservationEnvelope],
        provenance: impl Into<String>,
        utc_ms: i64,
    ) -> Result<ObservationChunkDescriptor> {
        self.publish_observation_chunk_with_cancel(
            observations,
            ObservationChunkProvenance::new(provenance.into())?,
            utc_ms,
            NeverCancel,
        )
    }

    pub fn publish_observation_chunk_with_cancel<C: Cancellation>(
        &mut self,
        observations: &[ObservationEnvelope],
        provenance: ObservationChunkProvenance,
        utc_ms: i64,
        cancel: C,
    ) -> Result<ObservationChunkDescriptor> {
        if self.mode == OpenMode::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        if utc_ms < 0 {
            return Err(StoreError::Invalid("publication time must be UTC".into()));
        }
        if !sqlite_guard::has_observation_chunk_schema(&self.connection)? {
            return Err(StoreError::Corrupt(
                "bundle lacks observation chunk metadata schema".into(),
            ));
        }
        self.start_operation()?;
        let prepared = canonical_observations(observations, &provenance, &cancel)?;
        check_cancel(&cancel)?;
        let hash = self.write_artifact_file(&prepared.bytes)?;
        if hash != prepared.descriptor.hash {
            return Err(StoreError::Corrupt(
                "artifact writer changed observation chunk hash".into(),
            ));
        }
        check_cancel(&cancel)?;

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let manifest = load_manifest(&transaction)?;
        if manifest.schema_version != crate::manifest::SCHEMA_VERSION
            || !manifest.required_features.is_empty()
        {
            return Err(StoreError::UnsupportedVersion(manifest.schema_version));
        }
        let existing = descriptor_query(&transaction, Some(&hash))?
            .into_iter()
            .next();
        if let Some(existing) = existing {
            if existing.provenance_id != provenance.provenance_id
                || existing.bytes != prepared.descriptor.bytes
                || existing.schema_version != prepared.descriptor.schema_version
                || existing.codec_version != prepared.descriptor.codec_version
                || existing.row_count != prepared.descriptor.row_count
            {
                return Err(StoreError::Corrupt(
                    "existing observation chunk metadata differs".into(),
                ));
            }
            drop(transaction);
            let rows = verify_descriptor(self, &existing)?;
            if rows != prepared.observations {
                return Err(StoreError::Corrupt(
                    "idempotent observation chunk bytes differ".into(),
                ));
            }
            return Ok(existing);
        }
        if manifest.artifacts.contains_key(&hash) {
            return Err(StoreError::Corrupt(
                "manifest references an observation artifact without metadata".into(),
            ));
        }

        let revision = manifest
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("revision exhausted".into()))?;
        let mut descriptor = prepared.descriptor.clone();
        descriptor.revision = revision;
        let entry = chunk_artifact_entry(&descriptor);
        if utc_ms < manifest.updated_utc_ms {
            return Err(StoreError::Invalid(
                "update wall time precedes committed manifest".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO observation_chunks (chunk_hash,bytes,media_type,schema_version,codec_version,row_count,first_observation_id,last_observation_id,known_utc_count,first_utc_ns,last_utc_ns,first_source_id,last_source_id,first_session_id,last_session_id,provenance_id,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                &descriptor.hash,
                i64::try_from(descriptor.bytes).map_err(|_| StoreError::Invalid("chunk size exceeds SQLite integer".into()))?,
                &descriptor.media_type,
                i64::from(descriptor.schema_version),
                i64::from(descriptor.codec_version),
                i64::try_from(descriptor.row_count).map_err(|_| StoreError::Invalid("chunk row count exceeds SQLite integer".into()))?,
                String::from(descriptor.first_observation_id),
                String::from(descriptor.last_observation_id),
                i64::try_from(descriptor.known_utc_count).map_err(|_| StoreError::Invalid("known UTC count exceeds SQLite integer".into()))?,
                descriptor.first_utc_ns,
                descriptor.last_utc_ns,
                String::from(descriptor.first_source_id),
                String::from(descriptor.last_source_id),
                String::from(descriptor.first_session_id),
                String::from(descriptor.last_session_id),
                &descriptor.provenance_id,
                i64::try_from(revision).map_err(|_| StoreError::Invalid("revision exhausted".into()))?,
            ],
        )?;
        for (ordinal, observation) in prepared.observations.iter().enumerate() {
            transaction.execute(
                "INSERT INTO observation_chunk_members (chunk_hash,observation_id,source_id,session_id,ordinal) VALUES (?1,?2,?3,?4,?5)",
                (
                    &descriptor.hash,
                    String::from(observation.data().id),
                    String::from(observation.data().source.source_id),
                    String::from(observation.data().session_id),
                    i64::try_from(ordinal).map_err(|_| StoreError::Invalid("observation ordinal exceeds SQLite integer".into()))?,
                ),
            )?;
        }
        let mut next_manifest = manifest;
        next_manifest.artifacts.insert(hash.clone(), entry);
        next_manifest.revision = revision;
        next_manifest.updated_utc_ms = utc_ms;
        let encoded = next_manifest.encode()?;
        let changed = transaction.execute(
            "UPDATE bundle_manifest SET revision=?1, body=?2 WHERE singleton=1",
            (
                i64::try_from(revision)
                    .map_err(|_| StoreError::Invalid("revision exhausted".into()))?,
                &encoded,
            ),
        )?;
        if changed != 1 {
            return Err(StoreError::Corrupt(
                "manifest update did not affect one row".into(),
            ));
        }
        check_cancel(&cancel)?;
        let readback = load_manifest(&transaction)?;
        if readback != next_manifest {
            return Err(StoreError::Corrupt(
                "observation manifest readback mismatch".into(),
            ));
        }
        let mut stored_statement = transaction
            .prepare("SELECT observation_id FROM observation_chunk_members WHERE chunk_hash=?1")?;
        let mut stored_rows = stored_statement.query([&descriptor.hash])?;
        let mut stored_count = 0_i64;
        while stored_rows.next()?.is_some() {
            stored_count = stored_count
                .checked_add(1)
                .ok_or_else(|| StoreError::Corrupt("observation member count overflow".into()))?;
        }
        drop(stored_rows);
        drop(stored_statement);
        if stored_count != i64::try_from(descriptor.row_count).unwrap_or(i64::MAX) {
            return Err(StoreError::Corrupt(
                "observation member count readback mismatch".into(),
            ));
        }
        let stored_descriptor = descriptor_query(&transaction, Some(&hash))?
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::Corrupt("observation metadata readback missing".into()))?;
        if stored_descriptor != descriptor {
            return Err(StoreError::Corrupt(
                "observation metadata readback mismatch".into(),
            ));
        }
        atomic_projection(&self.root, &next_manifest)?;
        transaction.commit()?;
        Ok(descriptor)
    }

    /// Alias for callers that use the general artifact vocabulary.
    pub fn put_observation_chunk(
        &mut self,
        observations: &[ObservationEnvelope],
        provenance: impl Into<String>,
        utc_ms: i64,
    ) -> Result<ObservationChunkDescriptor> {
        self.publish_observation_chunk(observations, provenance, utc_ms)
    }

    pub fn list_observation_chunks(&self) -> Result<Vec<ObservationChunkDescriptor>> {
        inventory(self)
    }

    pub fn read_observation_chunk(&self, hash: &str) -> Result<Vec<ObservationEnvelope>> {
        validate_hash(hash)?;
        let descriptors = inventory(self)?;
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.hash == hash)
            .ok_or_else(|| StoreError::Invalid("unregistered observation chunk".into()))?;
        verify_descriptor(self, descriptor)
    }

    pub fn read_observations(&self) -> Result<Vec<ObservationEnvelope>> {
        let descriptors = inventory(self)?;
        let mut result = Vec::new();
        for descriptor in &descriptors {
            if result.len() as u64 + descriptor.row_count > MAX_OBSERVATIONS_PER_READ {
                return Err(StoreError::Invalid(
                    "observation read exceeds materialization limit".into(),
                ));
            }
            result.extend(verify_descriptor(self, descriptor)?);
        }
        Ok(result)
    }

    pub(crate) fn verify_observation_chunks(&self) -> Result<()> {
        inventory(self).map(|_| ())
    }
}

impl ObservationChunkStore for Bundle {
    fn publish_observation_chunk(
        &mut self,
        observations: &[ObservationEnvelope],
        provenance: ObservationChunkProvenance,
        utc_ms: i64,
    ) -> Result<ObservationChunkDescriptor> {
        Bundle::publish_observation_chunk(self, observations, provenance, utc_ms)
    }

    fn list_observation_chunks(&self) -> Result<Vec<ObservationChunkDescriptor>> {
        Bundle::list_observation_chunks(self)
    }

    fn read_observation_chunk(&self, hash: &str) -> Result<Vec<ObservationEnvelope>> {
        Bundle::read_observation_chunk(self, hash)
    }
}
