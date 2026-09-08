use kyberia_domain::identity::{ObservationId, ProjectId, SessionId, SourceId};
use kyberia_project_store::{
    Bundle, MAX_OBSERVATION_CHUNK_BYTES, MAX_OBSERVATION_CHUNK_ROWS,
    OBSERVATION_CHUNK_CODEC_VERSION, OBSERVATION_SCHEMA_VERSION, ObservationChunkDescriptor,
    OpenMode, PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE, PARQUET_SCHEMA_FINGERPRINT,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

const OBSERVATION_EXPORT_SCHEMA: &str = "kyberia.observation-parquet-export/1";
const OBSERVATION_EXPORT_PRIVACY_WARNING: &str = "Normalized observations can contain MAC, SSID, client, location, or infrastructure identifiers. Review every row's privacy policy before sharing.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationParquetExportV1 {
    schema: String,
    project_id: ProjectId,
    project_revision: u64,
    observation_schema_version: u32,
    chunk_media_type: String,
    parquet_schema_fingerprint: String,
    privacy_warning: String,
    chunks: Vec<ObservationParquetExportChunkV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationParquetExportChunkV1 {
    hash: String,
    file_name: String,
    bytes: u64,
    media_type: String,
    observation_schema_version: u32,
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
    committed_revision: u64,
}

impl From<&ObservationChunkDescriptor> for ObservationParquetExportChunkV1 {
    fn from(chunk: &ObservationChunkDescriptor) -> Self {
        Self {
            hash: chunk.hash().to_owned(),
            file_name: format!("{}.parquet", chunk.hash()),
            bytes: chunk.bytes(),
            media_type: chunk.media_type().to_owned(),
            observation_schema_version: chunk.schema_version(),
            codec_version: chunk.codec_version(),
            row_count: chunk.row_count(),
            first_observation_id: chunk.first_observation_id(),
            last_observation_id: chunk.last_observation_id(),
            known_utc_count: chunk.known_utc_count(),
            first_utc_ns: chunk.first_utc_ns(),
            last_utc_ns: chunk.last_utc_ns(),
            first_source_id: chunk.first_source_id(),
            last_source_id: chunk.last_source_id(),
            first_session_id: chunk.first_session_id(),
            last_session_id: chunk.last_session_id(),
            provenance_id: chunk.provenance_id().to_owned(),
            committed_revision: chunk.revision(),
        }
    }
}

impl ObservationParquetExportV1 {
    fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.schema != OBSERVATION_EXPORT_SCHEMA
            || self.observation_schema_version != OBSERVATION_SCHEMA_VERSION
            || self.chunk_media_type != PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE
            || self.parquet_schema_fingerprint != PARQUET_SCHEMA_FINGERPRINT
            || self.privacy_warning != OBSERVATION_EXPORT_PRIVACY_WARNING
        {
            return Err("unsupported observation export contract".into());
        }
        if self.chunks.len() > 10_000 {
            return Err("observation export manifest exceeds chunk limit".into());
        }
        let mut previous: Option<&str> = None;
        for chunk in &self.chunks {
            let lowercase_hash = chunk.hash.len() == 64
                && chunk
                    .hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
            if !lowercase_hash
                || chunk.file_name != format!("{}.parquet", chunk.hash)
                || chunk.bytes == 0
                || chunk.bytes > MAX_OBSERVATION_CHUNK_BYTES
                || chunk.media_type != PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE
                || chunk.observation_schema_version != OBSERVATION_SCHEMA_VERSION
                || chunk.codec_version != OBSERVATION_CHUNK_CODEC_VERSION
                || chunk.row_count == 0
                || chunk.row_count > MAX_OBSERVATION_CHUNK_ROWS
                || chunk.first_observation_id > chunk.last_observation_id
                || chunk.first_source_id > chunk.last_source_id
                || chunk.first_session_id > chunk.last_session_id
                || chunk.known_utc_count > chunk.row_count
                || (chunk.known_utc_count == 0
                    && (chunk.first_utc_ns.is_some() || chunk.last_utc_ns.is_some()))
                || (chunk.known_utc_count > 0
                    && (chunk.first_utc_ns.is_none()
                        || chunk.last_utc_ns.is_none()
                        || chunk.first_utc_ns > chunk.last_utc_ns))
                || chunk.provenance_id.is_empty()
                || chunk.provenance_id.len() > 1024
                || chunk.provenance_id.chars().any(char::is_control)
                || chunk.committed_revision == 0
                || chunk.committed_revision > self.project_revision
                || previous.is_some_and(|hash| hash >= chunk.hash.as_str())
            {
                return Err("invalid observation export manifest".into());
            }
            previous = Some(&chunk.hash);
        }
        Ok(())
    }
}

fn sync_directory(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn export(
    project_path: &Path,
    destination: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let bundle = Bundle::open(project_path, OpenMode::ReadOnly)?;
    let project = bundle.manifest()?;
    let chunks = bundle.list_observation_chunks()?;
    fs::create_dir(destination)?;
    for chunk in &chunks {
        let bytes = bundle.read_observation_chunk_parquet(chunk.hash())?;
        write_new(
            &destination.join(format!("{}.parquet", chunk.hash())),
            &bytes,
        )?;
    }
    if bundle.manifest()? != project {
        return Err("project changed while observations were being exported".into());
    }
    let rows = chunks.iter().map(|chunk| chunk.row_count()).sum::<u64>();
    let mut export_chunks = chunks
        .iter()
        .map(ObservationParquetExportChunkV1::from)
        .collect::<Vec<_>>();
    export_chunks.sort_by(|left, right| left.hash.cmp(&right.hash));
    let manifest = ObservationParquetExportV1 {
        schema: OBSERVATION_EXPORT_SCHEMA.to_owned(),
        project_id: project.project_id,
        project_revision: project.revision,
        observation_schema_version: OBSERVATION_SCHEMA_VERSION,
        chunk_media_type: PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE.to_owned(),
        parquet_schema_fingerprint: PARQUET_SCHEMA_FINGERPRINT.to_owned(),
        privacy_warning: OBSERVATION_EXPORT_PRIVACY_WARNING.to_owned(),
        chunks: export_chunks,
    };
    manifest.validate()?;
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    manifest_bytes.push(b'\n');
    let decoded: ObservationParquetExportV1 = serde_json::from_slice(&manifest_bytes)?;
    decoded.validate()?;
    if decoded != manifest {
        return Err("observation export manifest changed during encoding".into());
    }
    sync_directory(destination)?;
    let pending_manifest = destination.join(".manifest.json.pending");
    write_new(&pending_manifest, &manifest_bytes)?;
    fs::rename(pending_manifest, destination.join("manifest.json"))?;
    sync_directory(destination)?;
    Ok(json!({
        "schema": OBSERVATION_EXPORT_SCHEMA,
        "destination": destination,
        "chunks": manifest.chunks.len(),
        "rows": rows,
        "privacy_warning": OBSERVATION_EXPORT_PRIVACY_WARNING,
    }))
}

#[cfg(test)]
mod export_contract_tests {
    use super::*;

    fn manifest() -> ObservationParquetExportV1 {
        let hash = "a".repeat(64);
        ObservationParquetExportV1 {
            schema: OBSERVATION_EXPORT_SCHEMA.to_owned(),
            project_id: ProjectId::from_bytes([1; 16]).unwrap(),
            project_revision: 7,
            observation_schema_version: OBSERVATION_SCHEMA_VERSION,
            chunk_media_type: PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE.to_owned(),
            parquet_schema_fingerprint: PARQUET_SCHEMA_FINGERPRINT.to_owned(),
            privacy_warning: OBSERVATION_EXPORT_PRIVACY_WARNING.to_owned(),
            chunks: vec![ObservationParquetExportChunkV1 {
                file_name: format!("{hash}.parquet"),
                hash,
                bytes: 4,
                media_type: PARQUET_OBSERVATION_CHUNK_MEDIA_TYPE.to_owned(),
                observation_schema_version: OBSERVATION_SCHEMA_VERSION,
                codec_version: OBSERVATION_CHUNK_CODEC_VERSION,
                row_count: 1,
                first_observation_id: ObservationId::from_bytes([2; 16]).unwrap(),
                last_observation_id: ObservationId::from_bytes([2; 16]).unwrap(),
                known_utc_count: 0,
                first_utc_ns: None,
                last_utc_ns: None,
                first_source_id: SourceId::from_bytes([3; 16]).unwrap(),
                last_source_id: SourceId::from_bytes([3; 16]).unwrap(),
                first_session_id: SessionId::from_bytes([4; 16]).unwrap(),
                last_session_id: SessionId::from_bytes([4; 16]).unwrap(),
                provenance_id: "contract-test/1".to_owned(),
                committed_revision: 7,
            }],
        }
    }

    #[test]
    fn v1_export_contract_is_closed_and_self_validating() {
        let current = manifest();
        current.validate().unwrap();
        let mut value = serde_json::to_value(&current).unwrap();
        value["unexpected"] = json!(true);
        assert!(serde_json::from_value::<ObservationParquetExportV1>(value).is_err());

        let mut future = manifest();
        future.schema = "kyberia.observation-parquet-export/2".to_owned();
        assert!(future.validate().is_err());

        let mut mismatched_file = manifest();
        mismatched_file.chunks[0].file_name = "other.parquet".to_owned();
        assert!(mismatched_file.validate().is_err());

        let mut control_provenance = manifest();
        control_provenance.chunks[0].provenance_id = "bad\nprovenance".to_owned();
        assert!(control_provenance.validate().is_err());

        let mut excessive = manifest();
        excessive.chunks = vec![excessive.chunks[0].clone(); 10_001];
        assert!(excessive.validate().is_err());
    }
}
