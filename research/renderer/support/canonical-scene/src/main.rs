use kyberia_domain::{
    analysis::{ExactU64, VersionedArtifact},
    evidence::{ArtifactReference, Evidence, UnknownReason},
    identity::{ContentHash, FloorId, FrameId, ObservationId, Text},
    units::{CoordinateMeters, Dbm, Meters},
};
use kyberia_rendering_scene::SceneDocument;
use kyberia_spatial_analysis::{
    Config, Extrapolation, Grid, InputEvidencePlane, Method, MetricDefinition, Model, Point2,
    Sample,
};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

const SOURCE_BYTES: &[u8] = b"kyberia-renderer-canonical-scene-v1\n";

fn ids() -> (FloorId, FrameId) {
    (
        FloorId::from_bytes([0x11; 16]).expect("nonzero floor id"),
        FrameId::from_bytes([0x22; 16]).expect("nonzero frame id"),
    )
}

fn point(x: f64, y: f64) -> Point2 {
    Point2 {
        x: CoordinateMeters::new(x).expect("finite x"),
        y: CoordinateMeters::new(y).expect("finite y"),
    }
}

fn sample(id: u8, x: f64, y: f64, rssi: f64) -> Sample {
    let (floor_id, frame_id) = ids();
    Sample {
        observation_id: ObservationId::from_bytes([id; 16]).expect("nonzero observation id"),
        floor_id,
        frame_id,
        position: point(x, y),
        value: Evidence::Known(Dbm::new(rssi).expect("finite RSSI")),
        position_covariance: Evidence::Unknown(UnknownReason::NotMeasured),
    }
}

fn source_reference() -> ArtifactReference {
    ArtifactReference {
        sha256: ContentHash::from_sha256(Sha256::digest(SOURCE_BYTES).into()),
        media_type: Text::new("application/x-kyberia-render-scene-fixture")
            .expect("valid source media type"),
        byte_length: SOURCE_BYTES.len() as u64,
    }
}

fn build_scene(method: &str) -> SceneDocument {
    let (definition, method) = match method {
        "point" => (
            MetricDefinition::signal_rssi().expect("built-in point RSSI metric"),
            Method::PointValue,
        ),
        "nearest" => (
            MetricDefinition::signal_rssi_nearest().expect("built-in nearest RSSI metric"),
            Method::Nearest,
        ),
        "idw" => (
            MetricDefinition::signal_rssi_idw().expect("built-in IDW RSSI metric"),
            Method::Idw { power: 2.0 },
        ),
        other => panic!("unsupported method {other}; expected point, nearest, or idw"),
    };
    let definition_bytes = definition
        .canonical_bytes()
        .expect("canonical metric bytes");
    let definition_artifact = VersionedArtifact {
        version: definition.version().clone(),
        sha256: ContentHash::from_sha256(Sha256::digest(&definition_bytes).into()),
        byte_length: ExactU64::new(definition_bytes.len() as u64),
        media_type: Text::new(kyberia_spatial_analysis::METRIC_DEFINITION_MEDIA_TYPE)
            .expect("metric media type"),
    };
    let binding = definition
        .bind(definition_artifact)
        .expect("metric artifact binding");
    let (floor_id, frame_id) = ids();
    let model = Model::new(
        kyberia_spatial_analysis::Inputs {
            floor_id,
            frame_id,
            evidence_plane: InputEvidencePlane::Synthetic,
            metric_definition: binding,
            source_artifact: source_reference(),
            samples: vec![
                sample(1, 1.5, 1.5, -42.0),
                sample(2, 6.5, 1.5, -58.0),
                sample(3, 3.5, 4.5, -47.0),
            ],
        },
        Config {
            method,
            support_radius: Meters::new(3.0).expect("positive support radius"),
            minimum_locations: 2,
            maximum_neighbors: 3,
            extrapolation: Extrapolation::Disabled,
        },
    )
    .expect("valid model");
    let tile = model
        .tile(
            Grid {
                floor_id,
                frame_id,
                origin: point(0.0, 0.0),
                resolution: Meters::new(1.0).expect("positive resolution"),
                column_offset: 0,
                row_offset: 0,
                width: 8,
                height: 6,
            },
            || false,
        )
        .expect("valid numeric tile");
    SceneDocument::from_verified_tile(&tile, SOURCE_BYTES).expect("verified renderer scene")
}

fn main() {
    let output = env::args().nth(1).expect("usage: generator <output.json>");
    let method_arg = env::args().nth(2);
    let method = method_arg.as_deref().unwrap_or("idw");
    let output = Path::new(&output);
    let document = build_scene(method);
    fs::write(output, document.canonical_bytes()).expect("write canonical scene");
    let hash = document.sha256().bytes();
    let hash = hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    println!(
        "generated {} bytes sha256={}",
        document.canonical_bytes().len(),
        hash
    );
}
