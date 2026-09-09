use crate::cancellation::Cancelled;
use kyberia_domain::{
    analysis::{ExactU64, VersionedArtifact},
    evidence::{ArtifactReference, Evidence},
    identity::{
        AdapterId, ContentHash, FloorId, FrameId, MacAddress, ProjectId, SessionId, SnapshotId,
        SourceId,
    },
    units::{CoordinateMeters, Meters},
};
use kyberia_observation_analysis::SelectionManifest;
use kyberia_project_store::{Bundle, Cancellation, OpenMode};
use kyberia_spatial_analysis::{
    Config as SpatialConfig, Extrapolation, Grid, InputEvidencePlane, Method, MetricDefinition,
    MetricDefinitionBinding, Point2, SpatialMethod,
};
use kyberia_stored_analysis::{
    MAX_OUTPUT_BYTES, SnapshotInput, StoredRssiAnalysisRequest, StoredRssiAnalysisResult, run,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

pub const OUTPUT_SCHEMA: &str = "kyberia.stored-rssi-analysis-cli/1";
pub const OUTPUT_FILE: &str = "analysis.json";
pub const MAX_REQUEST_BYTES: usize = 1_048_576;
pub const MAX_REQUEST_DEPTH: usize = 16;
pub const MAX_REQUEST_OBSERVATIONS: usize = kyberia_observation_analysis::MAX_OBSERVATIONS;
pub const MAX_REQUEST_SNAPSHOTS: usize = kyberia_stored_analysis::MAX_SNAPSHOT_INPUTS;
const PRIVACY_WARNING: &str = "The canonical artifact may contain observation, source, and location identifiers; review it before sharing.";

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RequestSchema {
    #[serde(rename = "kyberia.stored-rssi-analysis-request/1")]
    V1,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
enum MethodRequest {
    PointValue,
    Nearest,
    Idw { power: f64 },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
enum ExtrapolationRequest {
    Disabled,
    WithinRadius { radius_m: Meters },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotRequest {
    snapshot_id: SnapshotId,
    floor_id: FloorId,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GridRequest {
    origin_x_m: CoordinateMeters,
    origin_y_m: CoordinateMeters,
    resolution_m: Meters,
    column_offset: u32,
    row_offset: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestV1 {
    schema: RequestSchema,
    project_id: ProjectId,
    project_revision: ExactU64,
    floor_id: FloorId,
    frame_id: FrameId,
    target_bssid: MacAddress,
    observation_ids: Vec<kyberia_domain::identity::ObservationId>,
    snapshots: Vec<SnapshotRequest>,
    session_scope: Option<SessionId>,
    source_scope: Option<SourceId>,
    adapter_scope: Option<AdapterId>,
    allow_uncalibrated: bool,
    method: MethodRequest,
    support_radius_m: Meters,
    minimum_locations: u32,
    maximum_neighbors: u32,
    extrapolation: ExtrapolationRequest,
    grid: GridRequest,
}

impl RequestV1 {
    fn into_core(self) -> Result<StoredRssiAnalysisRequest, Box<dyn std::error::Error>> {
        if !matches!(self.schema, RequestSchema::V1) {
            return Err("unsupported stored RSSI request schema".into());
        }
        if self.observation_ids.len() > MAX_REQUEST_OBSERVATIONS {
            return Err("stored RSSI request exceeds observation limit".into());
        }
        if self.snapshots.len() > MAX_REQUEST_SNAPSHOTS {
            return Err("stored RSSI request exceeds snapshot limit".into());
        }
        let (method, spatial_method) = match self.method {
            MethodRequest::PointValue => (Method::PointValue, SpatialMethod::PointValue),
            MethodRequest::Nearest => (Method::Nearest, SpatialMethod::Nearest),
            MethodRequest::Idw { power } => (
                Method::Idw { power },
                SpatialMethod::InverseDistanceWeighted,
            ),
        };
        let extrapolation = match self.extrapolation {
            ExtrapolationRequest::Disabled => Extrapolation::Disabled,
            ExtrapolationRequest::WithinRadius { radius_m } => {
                Extrapolation::WithinRadius(radius_m)
            }
        };
        let metric = canonical_metric(spatial_method)?;
        let grid = Grid {
            floor_id: self.floor_id,
            frame_id: self.frame_id,
            origin: Point2 {
                x: self.grid.origin_x_m,
                y: self.grid.origin_y_m,
            },
            resolution: self.grid.resolution_m,
            column_offset: self.grid.column_offset,
            row_offset: self.grid.row_offset,
            width: self.grid.width,
            height: self.grid.height,
        };
        Ok(StoredRssiAnalysisRequest {
            project_id: self.project_id,
            project_revision: self.project_revision.get(),
            floor_id: self.floor_id,
            frame_id: self.frame_id,
            target_bssid: self.target_bssid,
            observation_ids: self.observation_ids,
            snapshots: self
                .snapshots
                .into_iter()
                .map(|snapshot| SnapshotInput {
                    snapshot_id: snapshot.snapshot_id,
                    floor_id: snapshot.floor_id,
                })
                .collect(),
            session_scope: self.session_scope,
            source_scope: self.source_scope,
            adapter_scope: self.adapter_scope,
            allow_uncalibrated: self.allow_uncalibrated,
            metric,
            spatial_configuration: SpatialConfig {
                method,
                support_radius: self.support_radius_m,
                minimum_locations: self.minimum_locations as usize,
                maximum_neighbors: self.maximum_neighbors as usize,
                extrapolation,
            },
            grid,
        })
    }
}

fn canonical_metric(
    spatial_method: SpatialMethod,
) -> Result<MetricDefinitionBinding, Box<dyn std::error::Error>> {
    let definition = MetricDefinition::observed_rssi(spatial_method)?;
    let bytes = definition.canonical_bytes()?;
    let artifact = VersionedArtifact {
        version: definition.version().clone(),
        sha256: ContentHash::from_sha256(Sha256::digest(&bytes).into()),
        byte_length: ExactU64::new(bytes.len() as u64),
        media_type: kyberia_domain::identity::Text::new(
            kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE,
        )?,
    };
    Ok(definition.bind(artifact)?)
}

fn read_request(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    #[cfg(not(unix))]
    {
        let _ = path;
        return Err(
            "stored RSSI request acquisition is unsupported on this platform; use a Unix regular-file adapter"
                .into(),
        );
    }
    #[cfg(unix)]
    {
        let mut options = OpenOptions::new();
        options.read(true);
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let mut file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err("stored RSSI request must be a regular file".into());
        }
        if metadata.len() > MAX_REQUEST_BYTES as u64 {
            return Err("stored RSSI request exceeds 1 MiB limit".into());
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        Read::by_ref(&mut file)
            .take((MAX_REQUEST_BYTES as u64) + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err("stored RSSI request exceeds 1 MiB limit".into());
        }
        validate_json_depth(&bytes)?;
        Ok(bytes)
    }
}

fn validate_json_depth(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth.checked_add(1).ok_or("request depth overflow")?;
                if depth > MAX_REQUEST_DEPTH {
                    return Err("stored RSSI request exceeds JSON depth limit".into());
                }
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Err("malformed stored RSSI request structure".into());
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    if in_string || escaped || depth != 0 {
        return Err("malformed stored RSSI request structure".into());
    }
    Ok(())
}

fn parse_request(path: &Path) -> Result<StoredRssiAnalysisRequest, Box<dyn std::error::Error>> {
    let bytes = read_request(path)?;
    let request: RequestV1 = serde_json::from_slice(&bytes)?;
    request.into_core()
}

#[derive(Clone, Debug, Serialize)]
struct CellSummary {
    total: usize,
    known: usize,
    unknown: usize,
    classes: BTreeMap<String, usize>,
    unknown_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
struct AnalysisReport {
    schema: &'static str,
    publication_status: &'static str,
    cancelled_after_commit: bool,
    project_id: ProjectId,
    project_revision: ExactU64,
    artifact: ArtifactReference,
    output_file: &'static str,
    evidence_plane: InputEvidencePlane,
    selected_observations: usize,
    rejected_observations: usize,
    cells: CellSummary,
    privacy_warning: &'static str,
}

fn cell_summary(
    result: &StoredRssiAnalysisResult,
) -> Result<CellSummary, Box<dyn std::error::Error>> {
    let mut classes = BTreeMap::new();
    let mut unknown_reasons = BTreeMap::new();
    let mut known = 0;
    for cell in &result.tile().cells {
        let class = match cell.class {
            kyberia_spatial_analysis::CellClass::Observed => "observed",
            kyberia_spatial_analysis::CellClass::Interpolated => "interpolated",
            kyberia_spatial_analysis::CellClass::Extrapolated => "extrapolated",
            kyberia_spatial_analysis::CellClass::Unknown => "unknown",
        };
        *classes.entry(class.to_owned()).or_insert(0) += 1;
        match &cell.value {
            Evidence::Known(_) => known += 1,
            Evidence::Unknown(reason) => {
                let name = serde_json::to_value(reason)?
                    .as_str()
                    .ok_or("unknown reason was not a string")?
                    .to_owned();
                *unknown_reasons.entry(name).or_insert(0) += 1;
            }
        }
    }
    Ok(CellSummary {
        total: result.tile().cells.len(),
        known,
        unknown: result.tile().cells.len() - known,
        classes,
        unknown_reasons,
    })
}

fn sync_directory(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[derive(Debug)]
pub(crate) struct PublicationDurabilityError {
    message: String,
}

impl PublicationDurabilityError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl std::fmt::Display for PublicationDurabilityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "publication committed but directory durability failed: {}",
            self.message
        )
    }
}

impl std::error::Error for PublicationDurabilityError {}

#[derive(Debug)]
enum Publication {
    Committed { cancelled_after_commit: bool },
}

fn check_cancel(cancel: &dyn Cancellation) -> Result<(), Cancelled> {
    if cancel.is_cancelled() {
        Err(Cancelled)
    } else {
        Ok(())
    }
}

fn publish(
    destination: &Path,
    bytes: &[u8],
    cancel: &dyn Cancellation,
) -> Result<Publication, Box<dyn std::error::Error>> {
    check_cancel(cancel)?;
    fs::create_dir(destination)?;
    publish_in_directory(destination, bytes, cancel)
}

fn publish_in_directory(
    destination: &Path,
    bytes: &[u8],
    cancel: &dyn Cancellation,
) -> Result<Publication, Box<dyn std::error::Error>> {
    publish_in_directory_with_sync(destination, bytes, cancel, sync_directory)
}

fn publish_in_directory_with_sync(
    destination: &Path,
    bytes: &[u8],
    cancel: &dyn Cancellation,
    sync: impl Fn(&Path) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<Publication, Box<dyn std::error::Error>> {
    check_cancel(cancel)?;
    let pending = destination.join(".analysis.json.pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    check_cancel(cancel)?;
    // A hard link is an atomic create-new publication on the same filesystem;
    // unlike rename, it cannot replace a final artifact that appeared after
    // the destination directory was created. The pending link is retained so
    // an operator can inspect or manually retire the exact committed bytes.
    fs::hard_link(&pending, destination.join(OUTPUT_FILE))?;
    sync(destination).map_err(|error| {
        Box::new(PublicationDurabilityError::new(error.to_string())) as Box<dyn std::error::Error>
    })?;
    Ok(Publication::Committed {
        cancelled_after_commit: cancel.is_cancelled(),
    })
}

pub fn analyze_with_cancellation(
    project_path: &Path,
    request_path: &Path,
    destination: &Path,
    cancel: &dyn Cancellation,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    check_cancel(cancel)?;
    let request = parse_request(request_path)?;
    check_cancel(cancel)?;
    let bundle = Bundle::open(project_path, OpenMode::ReadOnly)?;
    let result = run(&bundle, request, cancel).map_err(|error| match error {
        kyberia_stored_analysis::StoredAnalysisError::Cancelled => {
            Box::new(Cancelled) as Box<dyn std::error::Error>
        }
        other => Box::new(other) as Box<dyn std::error::Error>,
    })?;
    check_cancel(cancel)?;
    if result.canonical_bytes().len() > MAX_OUTPUT_BYTES {
        return Err("stored RSSI output exceeds resource limit".into());
    }
    let selection = SelectionManifest::from_canonical_bytes(result.selection_manifest())?;
    let mut report = AnalysisReport {
        schema: OUTPUT_SCHEMA,
        publication_status: "published",
        cancelled_after_commit: false,
        project_id: result.document().project_id,
        project_revision: result.document().project_revision,
        artifact: result.artifact().clone(),
        output_file: OUTPUT_FILE,
        evidence_plane: result.tile().inputs.evidence_plane,
        selected_observations: selection.selected.len(),
        rejected_observations: selection.rejected.len(),
        cells: cell_summary(&result)?,
        privacy_warning: PRIVACY_WARNING,
    };
    let publication = publish(destination, result.canonical_bytes(), cancel)?;
    match publication {
        Publication::Committed {
            cancelled_after_commit,
        } => {
            report.cancelled_after_commit = cancelled_after_commit;
            if cancelled_after_commit {
                report.publication_status = "published_after_cancellation";
            }
        }
    }
    let report_value = serde_json::to_value(&report)?;
    Ok(report_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn retained_test_directory() -> std::path::PathBuf {
        static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".trash")
            .join("test-runs");
        fs::create_dir_all(&root).unwrap();
        let process = std::process::id();
        loop {
            let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let candidate = root.join(format!("stored-rssi-cli-unit-{process}-{ordinal}"));
            match fs::create_dir(&candidate) {
                Ok(()) => return candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot retain test directory {candidate:?}: {error}"),
            }
        }
    }

    #[test]
    fn request_methods_bind_the_matching_canonical_registry_metric() {
        for (method, expected) in [
            (MethodRequest::PointValue, "wifi.rssi/1"),
            (MethodRequest::Nearest, "wifi.rssi.nearest/1"),
            (MethodRequest::Idw { power: 2.0 }, "wifi.rssi.idw/1"),
        ] {
            let metric = canonical_metric(match method {
                MethodRequest::PointValue => SpatialMethod::PointValue,
                MethodRequest::Nearest => SpatialMethod::Nearest,
                MethodRequest::Idw { .. } => SpatialMethod::InverseDistanceWeighted,
            })
            .unwrap();
            assert_eq!(metric.definition().version().as_str(), expected);
        }
    }

    #[test]
    fn request_depth_and_malformed_structure_are_rejected_before_serde() {
        assert!(validate_json_depth(b"{\"a\":[[[[[[[[[[[[[[[[0]]]]]]]]]]]]]]]]}").is_err());
        assert!(validate_json_depth(b"{\"a\": 1").is_err());
        assert!(validate_json_depth(b"{\"a\": \"unterminated}").is_err());
    }

    #[test]
    fn failed_retry_preserves_a_pending_publication_for_manual_recovery() {
        let root = retained_test_directory();
        let destination = root.join("output");
        fs::create_dir(&destination).unwrap();
        let pending = destination.join(".analysis.json.pending");
        fs::write(&pending, b"partial artifact").unwrap();

        assert!(
            publish(
                &destination,
                b"replacement",
                &kyberia_project_store::NeverCancel
            )
            .is_err()
        );
        assert_eq!(fs::read(&pending).unwrap(), b"partial artifact");
        assert!(!destination.join(OUTPUT_FILE).exists());
    }

    #[test]
    fn final_conflict_cannot_replace_existing_bytes() {
        let root = retained_test_directory();
        let destination = root.join("output");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join(OUTPUT_FILE), b"authoritative prior output").unwrap();

        assert!(
            publish_in_directory(
                &destination,
                b"replacement",
                &kyberia_project_store::NeverCancel
            )
            .is_err()
        );
        assert_eq!(
            fs::read(destination.join(OUTPUT_FILE)).unwrap(),
            b"authoritative prior output"
        );
        assert_eq!(
            fs::read(destination.join(".analysis.json.pending")).unwrap(),
            b"replacement"
        );
    }

    #[test]
    fn cancellation_before_publication_leaves_no_final_artifact() {
        let root = retained_test_directory();
        let destination = root.join("output");
        let token = crate::cancellation::CancellationToken::default();
        token.cancel();

        assert!(publish(&destination, b"cancelled", &token).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn cancellation_after_pending_sync_retains_pending_without_final() {
        let root = retained_test_directory();
        let destination = root.join("output");
        let checks = AtomicUsize::new(0);
        let cancel = || checks.fetch_add(1, Ordering::SeqCst) >= 2;

        assert!(publish(&destination, b"cancelled", &cancel).is_err());
        assert_eq!(
            fs::read(destination.join(".analysis.json.pending")).unwrap(),
            b"cancelled"
        );
        assert!(!destination.join(OUTPUT_FILE).exists());
    }

    #[test]
    fn cancellation_after_final_link_reports_committed_outcome() {
        let root = retained_test_directory();
        let destination = root.join("output");
        let checks = AtomicUsize::new(0);
        let cancel = || checks.fetch_add(1, Ordering::SeqCst) >= 3;

        assert!(matches!(
            publish(&destination, b"committed", &cancel),
            Ok(Publication::Committed {
                cancelled_after_commit: true
            })
        ));
        assert_eq!(
            fs::read(destination.join(OUTPUT_FILE)).unwrap(),
            b"committed"
        );
    }

    #[test]
    fn directory_durability_failure_preserves_committed_outcome() {
        let root = retained_test_directory();
        let destination = root.join("output");
        fs::create_dir(&destination).unwrap();
        let result = publish_in_directory_with_sync(
            &destination,
            b"committed-before-sync-failure",
            &kyberia_project_store::NeverCancel,
            |_| Err("injected directory sync failure".into()),
        );
        let error = result.unwrap_err();
        let durability = error
            .downcast_ref::<PublicationDurabilityError>()
            .expect("post-commit failures retain their committed error type");
        assert!(durability.to_string().contains("directory sync failure"));
        assert_eq!(
            fs::read(destination.join(OUTPUT_FILE)).unwrap(),
            b"committed-before-sync-failure"
        );
        assert_eq!(
            fs::read(destination.join(".analysis.json.pending")).unwrap(),
            b"committed-before-sync-failure"
        );
    }
}
