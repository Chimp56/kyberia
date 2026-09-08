//! Bounded, typed Arrow/Parquet encoding for canonical observation envelopes.
//!
//! The domain remains independent of Arrow and Parquet. This adapter flattens
//! the domain's JSON *shape* into one typed Arrow column per scalar leaf (and
//! typed list columns for arrays), then reconstructs the domain value after
//! reading. JSON is used only as an in-memory structural bridge; no JSON bytes
//! are stored in the Parquet artifact.

use crate::{
    MAX_OBSERVATION_CHUNK_BYTES, MAX_OBSERVATION_CHUNK_ROWS, MAX_OBSERVATION_ROW_BYTES, Result,
    StoreError,
};
use arrow_array::builder::{
    BooleanBuilder, Float64Builder, Int64Builder, ListBuilder, StringBuilder, UInt64Builder,
};
use arrow_array::{
    Array, ArrayRef, BooleanArray, Float64Array, Int64Array, ListArray, RecordBatch, StringArray,
    UInt64Array,
};
use arrow_schema::{DataType, Field, Schema};
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::arrow_writer::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::sync::Arc;

pub const FORMAT_VERSION: u32 = 2;
pub const CODEC_VERSION: u32 = 2;
pub const MEDIA_TYPE: &str = "application/vnd.apache.parquet; profile=kyberia-observation-v2";
pub const SCHEMA_FINGERPRINT: &str = "kyberia-envelope-v2-fixed-superset-1";

const METADATA_KEY: &str = "kyberia_observation_chunk";
const METADATA_PREFIX: &str = "version=2;schema=kyberia-envelope-v2-fixed-superset-1;encoding=typed-column-paths;object-lists=";
const UNION_SUFFIX: &str = "__";
const MAX_COLUMNS: usize = 256;
const MAX_BATCH_SIZE: usize = 1024;
const MAX_LIST_VALUES: usize = 1024;
const MAX_STRING_BYTES: usize = 1024;
const MAX_ROW_GROUPS: usize = 64;
const MAX_DECODED_ARROW_BYTES: u64 = MAX_OBSERVATION_CHUNK_BYTES;

#[derive(Clone, Debug, PartialEq)]
enum Scalar {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
}

#[derive(Clone, Debug, PartialEq)]
enum FlatValue {
    Scalar(Scalar),
    List(Vec<Option<Scalar>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Bool,
    I64,
    U64,
    F64,
    String,
}

impl Kind {
    fn data_type(self) -> DataType {
        match self {
            Self::Bool => DataType::Boolean,
            Self::I64 => DataType::Int64,
            Self::U64 => DataType::UInt64,
            Self::F64 => DataType::Float64,
            Self::String => DataType::Utf8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ColumnKind {
    Scalar(Kind),
    List(Kind),
}

type LogicalColumns = BTreeMap<String, BTreeSet<ColumnKind>>;
type PhysicalColumns = BTreeMap<String, (String, ColumnKind)>;

impl ColumnKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Scalar(Kind::Bool) => "scalar_bool",
            Self::Scalar(Kind::I64) => "scalar_i64",
            Self::Scalar(Kind::U64) => "scalar_u64",
            Self::Scalar(Kind::F64) => "scalar_f64",
            Self::Scalar(Kind::String) => "scalar_string",
            Self::List(Kind::Bool) => "list_bool",
            Self::List(Kind::I64) => "list_i64",
            Self::List(Kind::U64) => "list_u64",
            Self::List(Kind::F64) => "list_f64",
            Self::List(Kind::String) => "list_string",
        }
    }
}

#[derive(Default)]
struct BoundedWriter {
    bytes: Vec<u8>,
}

impl Write for BoundedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let next = self
            .bytes
            .len()
            .checked_add(data.len())
            .ok_or_else(|| std::io::Error::other("Parquet output length overflow"))?;
        if next as u64 > MAX_OBSERVATION_CHUNK_BYTES {
            return Err(std::io::Error::other(
                "Parquet output exceeds observation chunk byte limit",
            ));
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn codec_error(error: impl std::fmt::Display) -> StoreError {
    StoreError::ChunkCodec(error.to_string())
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

fn unsigned_semantic(path: &str) -> bool {
    matches!(
        path.rsplit('.').next().unwrap_or(path),
        "nanoseconds"
            | "reference_monotonic_nanoseconds"
            | "cycle_index"
            | "byte_length"
            | "dropped_events"
            | "queued_events"
    )
}

fn scalar(path: &str, value: &Value) -> Result<Scalar> {
    match value {
        Value::Bool(value) => Ok(Scalar::Bool(*value)),
        Value::Number(number) => {
            if unsigned_semantic(path) {
                let value = number.as_u64().ok_or_else(|| {
                    StoreError::Corrupt(format!("negative unsigned Parquet value at {path}"))
                })?;
                Ok(Scalar::U64(value))
            } else if let Some(value) = number.as_i64() {
                Ok(Scalar::I64(value))
            } else if let Some(value) = number.as_u64() {
                Ok(Scalar::U64(value))
            } else if let Some(value) = number.as_f64() {
                if value.is_finite() {
                    Ok(Scalar::F64(value))
                } else {
                    Err(StoreError::Corrupt("non-finite Parquet scalar".into()))
                }
            } else {
                Err(StoreError::Corrupt("unsupported Parquet number".into()))
            }
        }
        Value::String(value) => Ok(Scalar::String(value.clone())),
        Value::Null => Err(StoreError::Corrupt(
            "null is not a canonical observation value".into(),
        )),
        Value::Array(_) | Value::Object(_) => Err(StoreError::Corrupt(
            "nested Parquet value was not flattened".into(),
        )),
    }
}

fn scalar_kind(value: &Scalar) -> Kind {
    match value {
        Scalar::Bool(_) => Kind::Bool,
        Scalar::I64(_) => Kind::I64,
        Scalar::U64(_) => Kind::U64,
        Scalar::F64(_) => Kind::F64,
        Scalar::String(_) => Kind::String,
    }
}

fn flatten(
    path: &str,
    value: &Value,
    output: &mut BTreeMap<String, FlatValue>,
    object_lists: &mut BTreeSet<String>,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (name, value) in object {
                let child = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}.{name}")
                };
                flatten(&child, value, output, object_lists)?;
            }
            Ok(())
        }
        Value::Array(values) => {
            if values.is_empty() && path.ends_with(".chains") {
                object_lists.insert(path.to_owned());
                Ok(())
            } else if values
                .iter()
                .all(|value| !value.is_object() && !value.is_array())
            {
                let values = values
                    .iter()
                    .map(|value| scalar(path, value))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .map(Some)
                    .collect();
                output.insert(path.to_owned(), FlatValue::List(values));
                Ok(())
            } else if values.iter().all(Value::is_object) {
                if values.is_empty() {
                    object_lists.insert(path.to_owned());
                    return Ok(());
                }
                object_lists.insert(path.to_owned());
                let object_count = values.len();
                let mut children: BTreeMap<String, Vec<Option<Scalar>>> = BTreeMap::new();
                for value in values {
                    let object = value.as_object().ok_or_else(|| {
                        StoreError::Corrupt("mixed Parquet list element types".into())
                    })?;
                    let mut row = BTreeMap::new();
                    for (name, child) in object {
                        flatten(name, child, &mut row, &mut BTreeSet::new())?;
                    }
                    for (name, child) in row {
                        let value = match child {
                            FlatValue::Scalar(value) => value,
                            FlatValue::List(_) => {
                                return Err(StoreError::Corrupt(
                                    "nested list in observation object list".into(),
                                ));
                            }
                        };
                        children.entry(name).or_default().push(Some(value));
                    }
                }
                if children.is_empty()
                    || children.values().any(|values| values.len() != object_count)
                {
                    return Err(StoreError::Corrupt("malformed Parquet object list".into()));
                }
                for (name, values) in children {
                    output.insert(format!("{path}.{name}"), FlatValue::List(values));
                }
                Ok(())
            } else {
                Err(StoreError::Corrupt(
                    "mixed Parquet list element types".into(),
                ))
            }
        }
        _ => {
            output.insert(path.to_owned(), FlatValue::Scalar(scalar(path, value)?));
            Ok(())
        }
    }
}

fn empty_list_kind(path: &str) -> Option<Kind> {
    if path == "quality" || path.ends_with(".quality") {
        Some(Kind::String)
    } else if path == "ssid.detail" || path.ends_with(".ssid.detail") {
        Some(Kind::U64)
    } else {
        None
    }
}

fn merge_kinds<'a>(
    path: &str,
    values: impl Iterator<Item = Option<&'a FlatValue>>,
) -> Result<BTreeSet<ColumnKind>> {
    let mut result = BTreeSet::new();
    for value in values.flatten() {
        match value {
            FlatValue::Scalar(value) => {
                result.insert(ColumnKind::Scalar(scalar_kind(value)));
            }
            FlatValue::List(values) => {
                let mut item_kinds = values
                    .iter()
                    .flatten()
                    .map(scalar_kind)
                    .collect::<BTreeSet<_>>();
                if item_kinds.is_empty() {
                    item_kinds.insert(empty_list_kind(path).ok_or_else(|| {
                        StoreError::Corrupt(format!(
                            "empty Parquet list {path} has no type witness"
                        ))
                    })?);
                }
                result.extend(item_kinds.into_iter().map(ColumnKind::List));
            }
        }
    }
    if result.is_empty() {
        return Err(StoreError::Corrupt(format!(
            "Parquet column {path} has no values"
        )));
    }
    Ok(result)
}

fn append_scalar(
    builder: &mut dyn std::any::Any,
    value: Option<&Scalar>,
    expected: Kind,
) -> Result<()> {
    let Some(value) = value else {
        return append_null(builder, expected);
    };
    match (expected, value) {
        (Kind::Bool, Scalar::Bool(value)) => builder
            .downcast_mut::<BooleanBuilder>()
            .ok_or_else(|| codec_error("BooleanBuilder type mismatch"))?
            .append_value(*value),
        (Kind::I64, Scalar::I64(value)) => builder
            .downcast_mut::<Int64Builder>()
            .ok_or_else(|| codec_error("Int64Builder type mismatch"))?
            .append_value(*value),
        (Kind::U64, Scalar::U64(value)) => builder
            .downcast_mut::<UInt64Builder>()
            .ok_or_else(|| codec_error("UInt64Builder type mismatch"))?
            .append_value(*value),
        (Kind::F64, Scalar::F64(value)) => builder
            .downcast_mut::<Float64Builder>()
            .ok_or_else(|| codec_error("Float64Builder type mismatch"))?
            .append_value(*value),
        (Kind::String, Scalar::String(value)) => builder
            .downcast_mut::<StringBuilder>()
            .ok_or_else(|| codec_error("StringBuilder type mismatch"))?
            .append_value(value),
        _ => append_null(builder, expected)?,
    }
    Ok(())
}

fn append_null(builder: &mut dyn std::any::Any, expected: Kind) -> Result<()> {
    match expected {
        Kind::Bool => builder
            .downcast_mut::<BooleanBuilder>()
            .ok_or_else(|| codec_error("BooleanBuilder type mismatch"))?
            .append_null(),
        Kind::I64 => builder
            .downcast_mut::<Int64Builder>()
            .ok_or_else(|| codec_error("Int64Builder type mismatch"))?
            .append_null(),
        Kind::U64 => builder
            .downcast_mut::<UInt64Builder>()
            .ok_or_else(|| codec_error("UInt64Builder type mismatch"))?
            .append_null(),
        Kind::F64 => builder
            .downcast_mut::<Float64Builder>()
            .ok_or_else(|| codec_error("Float64Builder type mismatch"))?
            .append_null(),
        Kind::String => builder
            .downcast_mut::<StringBuilder>()
            .ok_or_else(|| codec_error("StringBuilder type mismatch"))?
            .append_null(),
    }
    Ok(())
}

fn build_array(
    path: &str,
    column_kind: ColumnKind,
    rows: &[BTreeMap<String, FlatValue>],
) -> Result<ArrayRef> {
    match column_kind {
        ColumnKind::Scalar(kind) => match kind {
            Kind::Bool => Ok(Arc::new(BooleanArray::from(
                rows.iter()
                    .map(|row| match row.get(path) {
                        Some(FlatValue::Scalar(Scalar::Bool(value))) => Some(*value),
                        None => None,
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ))),
            Kind::I64 => Ok(Arc::new(Int64Array::from(
                rows.iter()
                    .map(|row| match row.get(path) {
                        Some(FlatValue::Scalar(Scalar::I64(value))) => Some(*value),
                        None => None,
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ))),
            Kind::U64 => Ok(Arc::new(UInt64Array::from(
                rows.iter()
                    .map(|row| match row.get(path) {
                        Some(FlatValue::Scalar(Scalar::U64(value))) => Some(*value),
                        None => None,
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ))),
            Kind::F64 => Ok(Arc::new(Float64Array::from(
                rows.iter()
                    .map(|row| match row.get(path) {
                        Some(FlatValue::Scalar(Scalar::F64(value))) => Some(*value),
                        None => None,
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
            ))),
            Kind::String => {
                let mut builder = StringBuilder::with_capacity(rows.len(), rows.len() * 16);
                for row in rows {
                    match row.get(path) {
                        Some(FlatValue::Scalar(Scalar::String(value))) => {
                            builder.append_value(value)
                        }
                        None => builder.append_null(),
                        _ => builder.append_null(),
                    }
                }
                Ok(Arc::new(builder.finish()))
            }
        },
        ColumnKind::List(kind) => match kind {
            Kind::Bool => {
                let field = Arc::new(Field::new("item", DataType::Boolean, true));
                let mut builder = ListBuilder::new(BooleanBuilder::new()).with_field(field);
                for row in rows {
                    match row.get(path) {
                        Some(FlatValue::List(values)) => {
                            let matched = values
                                .iter()
                                .flatten()
                                .any(|value| scalar_kind(value) == kind);
                            if matched || values.is_empty() {
                                for value in values {
                                    append_scalar(builder.values(), value.as_ref(), kind)?;
                                }
                                builder.append(true);
                            } else {
                                builder.append(false);
                            }
                        }
                        None => builder.append(false),
                        _ => builder.append(false),
                    }
                }
                Ok(Arc::new(builder.finish()))
            }
            Kind::I64 => {
                let field = Arc::new(Field::new("item", DataType::Int64, true));
                let mut builder = ListBuilder::new(Int64Builder::new()).with_field(field);
                for row in rows {
                    match row.get(path) {
                        Some(FlatValue::List(values)) => {
                            let matched = values
                                .iter()
                                .flatten()
                                .any(|value| scalar_kind(value) == kind);
                            if matched || values.is_empty() {
                                for value in values {
                                    append_scalar(builder.values(), value.as_ref(), kind)?;
                                }
                                builder.append(true);
                            } else {
                                builder.append(false);
                            }
                        }
                        None => builder.append(false),
                        _ => builder.append(false),
                    }
                }
                Ok(Arc::new(builder.finish()))
            }
            Kind::U64 => {
                let field = Arc::new(Field::new("item", DataType::UInt64, true));
                let mut builder = ListBuilder::new(UInt64Builder::new()).with_field(field);
                for row in rows {
                    match row.get(path) {
                        Some(FlatValue::List(values)) => {
                            let matched = values
                                .iter()
                                .flatten()
                                .any(|value| scalar_kind(value) == kind);
                            if matched || values.is_empty() {
                                for value in values {
                                    append_scalar(builder.values(), value.as_ref(), kind)?;
                                }
                                builder.append(true);
                            } else {
                                builder.append(false);
                            }
                        }
                        None => builder.append(false),
                        _ => builder.append(false),
                    }
                }
                Ok(Arc::new(builder.finish()))
            }
            Kind::F64 => {
                let field = Arc::new(Field::new("item", DataType::Float64, true));
                let mut builder = ListBuilder::new(Float64Builder::new()).with_field(field);
                for row in rows {
                    match row.get(path) {
                        Some(FlatValue::List(values)) => {
                            let matched = values
                                .iter()
                                .flatten()
                                .any(|value| scalar_kind(value) == kind);
                            if matched || values.is_empty() {
                                for value in values {
                                    append_scalar(builder.values(), value.as_ref(), kind)?;
                                }
                                builder.append(true);
                            } else {
                                builder.append(false);
                            }
                        }
                        None => builder.append(false),
                        _ => builder.append(false),
                    }
                }
                Ok(Arc::new(builder.finish()))
            }
            Kind::String => {
                let field = Arc::new(Field::new("item", DataType::Utf8, true));
                let mut builder = ListBuilder::new(StringBuilder::new()).with_field(field);
                for row in rows {
                    match row.get(path) {
                        Some(FlatValue::List(values)) => {
                            let matched = values
                                .iter()
                                .flatten()
                                .any(|value| scalar_kind(value) == kind);
                            if matched || values.is_empty() {
                                for value in values {
                                    append_scalar(builder.values(), value.as_ref(), kind)?;
                                }
                                builder.append(true);
                            } else {
                                builder.append(false);
                            }
                        }
                        None => builder.append(false),
                        _ => builder.append(false),
                    }
                }
                Ok(Arc::new(builder.finish()))
            }
        },
    }
}

// This checked-in witness is the closed V2 analytical contract. It is used to
// build the nullable superset before any input rows are inspected; adding or
// changing a field requires a new schema fingerprint and compatibility policy.
fn schema_unknown() -> Value {
    serde_json::json!({"state": "unknown", "detail": "not_measured"})
}

fn schema_known(value: Value) -> Value {
    serde_json::json!({"state": "known", "detail": value})
}

fn schema_evidence(nested_known: bool, value: Value) -> Value {
    if nested_known {
        schema_known(value)
    } else {
        schema_unknown()
    }
}

fn schema_id() -> &'static str {
    "01010101010101010101010101010101"
}

fn schema_hash() -> &'static str {
    "0202020202020202020202020202020202020202020202020202020202020202"
}

fn schema_artifact_reference() -> Value {
    serde_json::json!({
        "sha256": schema_hash(),
        "media_type": "application/octet-stream",
        "byte_length": 1,
    })
}

fn schema_source(nested_known: bool) -> Value {
    serde_json::json!({
        "source_id": schema_id(),
        "collector_id": schema_id(),
        "sensor_id": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "adapter_id": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "kind": "native_api",
        "source_name": "schema-source",
        "source_version": schema_evidence(nested_known, serde_json::json!("source/1")),
        "source_schema_version": "source-wire/2",
        "adapter_name": "schema-adapter",
        "adapter_version": "adapter/1",
        "parser_version": "parser/1",
        "driver_version": schema_evidence(nested_known, serde_json::json!("driver/1")),
        "os_version": schema_evidence(nested_known, serde_json::json!("os/1")),
    })
}

fn schema_channel(nested_known: bool) -> Value {
    serde_json::json!({
        "band": schema_evidence(nested_known, serde_json::json!("ghz2_4")),
        "primary_channel": schema_evidence(nested_known, serde_json::json!(1)),
        "primary_frequency": schema_evidence(nested_known, serde_json::json!(2.4)),
        "center_frequency": schema_evidence(nested_known, serde_json::json!(2.4)),
        "second_center_frequency": schema_evidence(nested_known, serde_json::json!(5.2)),
        "width": schema_evidence(nested_known, serde_json::json!(20.0)),
        "puncturing": schema_evidence(nested_known, serde_json::json!(0)),
    })
}

fn schema_dwell(nested_known: bool) -> Value {
    serde_json::json!({
        "schedule_id": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "cycle_index": schema_evidence(nested_known, serde_json::json!(1)),
        "tuned_channel": schema_channel(nested_known),
        "window": schema_evidence(nested_known, serde_json::json!({
            "start": {"epoch": schema_id(), "nanoseconds": 1},
            "end": {"epoch": schema_id(), "nanoseconds": 2},
        })),
        "reported_duration": schema_evidence(nested_known, serde_json::json!(1.0)),
        "method_version": "dwell/1",
    })
}

fn schema_pose(nested_known: bool) -> Value {
    serde_json::json!({
        "pose_id": schema_id(),
        "frame_id": schema_id(),
        "assignment_version": "assignment/1",
        "position": {"x": 1.0, "y": 2.0, "z": 3.0},
        "covariance": schema_evidence(nested_known, serde_json::json!([1.0, 0.0, 0.0, 1.0, 0.0, 1.0])),
        "orientation": schema_evidence(nested_known, serde_json::json!({
            "yaw": 0.1,
            "pitch": 0.2,
            "roll": 0.3,
        })),
        "method_version": "pose/1",
    })
}

fn schema_time(outer_known: bool, nested_known: bool) -> Value {
    let evidence = |value: Value| {
        if outer_known {
            schema_known(value)
        } else {
            schema_unknown()
        }
    };
    serde_json::json!({
        "wall": evidence(serde_json::json!({
            "time": 1,
            "source": "clock",
            "precision": 0.001,
            "uncertainty": schema_evidence(nested_known, serde_json::json!(0.001)),
        })),
        "monotonic": evidence(serde_json::json!({
            "epoch": schema_id(),
            "nanoseconds": 1,
        })),
        "synchronization": evidence(serde_json::json!({
            "epoch": schema_id(),
            "reference_monotonic_nanoseconds": 1,
            "reference_utc": 1,
            "offset_to_reference": schema_evidence(nested_known, serde_json::json!(0.0)),
            "drift": schema_evidence(nested_known, serde_json::json!(0.0)),
            "error": schema_evidence(nested_known, serde_json::json!(0.001)),
            "method_version": "clock/1",
        })),
    })
}

fn schema_identity(nested_known: bool) -> Value {
    serde_json::json!({
        "physical_device": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "radio": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "bss": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "bssid": schema_evidence(nested_known, serde_json::json!([1, 2, 3, 4, 5, 6])),
        "ess": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "mld": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "link_id": schema_evidence(nested_known, serde_json::json!(1)),
        "client": schema_evidence(nested_known, serde_json::json!(schema_id())),
        "grouping_evidence": schema_evidence(nested_known, schema_artifact_reference()),
    })
}

fn schema_chain(nested_known: bool, chain_index: u8) -> Value {
    serde_json::json!({
        "chain_index": chain_index,
        "rssi_dbm": schema_evidence(nested_known, serde_json::json!(-55.0)),
        "noise_dbm": schema_evidence(nested_known, serde_json::json!(-90.0)),
    })
}

fn schema_signal(nested_known: bool) -> Value {
    serde_json::json!({
        "rssi_dbm": schema_evidence(nested_known, serde_json::json!(-55.0)),
        "noise_dbm": schema_evidence(nested_known, serde_json::json!(-90.0)),
        "chains": [schema_chain(nested_known, 0), schema_chain(nested_known, 1)],
        "calibration": schema_evidence(nested_known, serde_json::json!({
            "state": "reference",
            "id": schema_id(),
            "version": "calibration/1",
        })),
        "measurement_method": "schema-measurement",
    })
}

fn schema_scan(nested_known: bool) -> Value {
    serde_json::json!({
        "kind": "scan",
        "data": {
            "identity": schema_identity(nested_known),
            "ssid": schema_evidence(nested_known, serde_json::json!([1, 2, 3])),
            "signal": schema_signal(nested_known),
            "information_elements": schema_evidence(nested_known, schema_artifact_reference()),
            "result_age": schema_evidence(nested_known, serde_json::json!(1.0)),
        },
    })
}

fn schema_frame(nested_known: bool) -> Value {
    serde_json::json!({
        "kind": "frame",
        "data": {
            "identity": schema_identity(nested_known),
            "signal": schema_signal(nested_known),
            "frame_type": schema_evidence(nested_known, serde_json::json!(1)),
            "frame_subtype": schema_evidence(nested_known, serde_json::json!(2)),
            "retry": schema_evidence(nested_known, serde_json::json!(false)),
            "length_bytes": 128,
            "phy_rate_mbps": schema_evidence(nested_known, serde_json::json!(1.0)),
            "raw_information_elements": schema_evidence(nested_known, schema_artifact_reference()),
        },
    })
}

fn schema_health(nested_known: bool) -> Value {
    serde_json::json!({
        "kind": "health",
        "data": {
            "dropped_events": schema_evidence(nested_known, serde_json::json!(1)),
            "queued_events": schema_evidence(nested_known, serde_json::json!(2)),
            "connected": schema_evidence(nested_known, serde_json::json!(true)),
            "diagnostic": "schema-health",
        },
    })
}

fn schema_envelope(top_known: bool, nested_known: bool, payload: Value) -> Value {
    let evidence = |value: Value| {
        if top_known {
            schema_known(value)
        } else {
            schema_unknown()
        }
    };
    serde_json::json!({
        "schema_version": "2",
        "id": schema_id(),
        "session_id": schema_id(),
        "source": schema_source(nested_known),
        "time": schema_time(top_known, nested_known),
        "pose": evidence(schema_pose(nested_known)),
        "channel": evidence(schema_channel(nested_known)),
        "dwell": evidence(schema_dwell(nested_known)),
        "privacy": {
            "policy_version": "privacy/1",
            "identifiers": "owned_infrastructure",
            "payload": {"state": "retained", "authorization_reference": "auth/1", "retention_deadline": 1},
        },
        "quality": ["clock_uncertain", "partial_capture"],
        "raw_source": evidence(schema_artifact_reference()),
        "payload": payload,
    })
}

fn schema_witness_rows() -> Vec<Value> {
    let mut rows = Vec::new();
    for payload in [
        schema_scan as fn(bool) -> Value,
        schema_frame,
        schema_health,
    ] {
        for top_known in [false, true] {
            for nested_known in [false, true] {
                rows.push(schema_envelope(
                    top_known,
                    nested_known,
                    payload(nested_known),
                ));
            }
        }
    }
    rows
}

fn collect_logical_columns(
    rows: &[BTreeMap<String, FlatValue>],
    logical_columns: &mut BTreeMap<String, BTreeSet<ColumnKind>>,
) -> Result<()> {
    for row in rows {
        for path in row.keys() {
            if !valid_path(path) {
                return Err(StoreError::Corrupt(
                    "invalid generated Parquet field path".into(),
                ));
            }
            let values = rows.iter().map(|candidate| candidate.get(path));
            logical_columns
                .entry(path.clone())
                .or_default()
                .extend(merge_kinds(path, values)?);
        }
    }
    Ok(())
}

fn fixed_contract() -> Result<(LogicalColumns, BTreeSet<String>)> {
    let witness_values = schema_witness_rows();
    let mut witness_rows = Vec::with_capacity(witness_values.len());
    let mut object_lists = BTreeSet::new();
    for value in witness_values {
        let mut row = BTreeMap::new();
        flatten("", &value, &mut row, &mut object_lists)?;
        witness_rows.push(row);
    }
    let mut logical_columns = LogicalColumns::new();
    collect_logical_columns(&witness_rows, &mut logical_columns)?;
    Ok((logical_columns, object_lists))
}

fn physical_columns(
    logical_columns: &LogicalColumns,
) -> Result<(BTreeSet<String>, PhysicalColumns)> {
    let union_paths = logical_columns
        .iter()
        .filter_map(|(path, kinds)| (kinds.len() > 1).then_some(path.clone()))
        .collect::<BTreeSet<_>>();
    let columns = logical_columns
        .iter()
        .flat_map(|(path, kinds)| {
            let union = union_paths.contains(path);
            kinds.iter().map(move |column_kind| {
                let physical_path = if union {
                    format!("{path}{UNION_SUFFIX}{}", column_kind.tag())
                } else {
                    path.clone()
                };
                (physical_path, (path.clone(), *column_kind))
            })
        })
        .collect::<BTreeMap<_, _>>();
    if columns.is_empty() || columns.len() > MAX_COLUMNS {
        return Err(StoreError::Invalid(
            "observation Parquet schema exceeds column limit".into(),
        ));
    }
    Ok((union_paths, columns))
}

pub fn encode(
    observations: &[kyberia_domain::observation::ObservationEnvelope],
) -> Result<Vec<u8>> {
    if observations.is_empty() || observations.len() as u64 > MAX_OBSERVATION_CHUNK_ROWS {
        return Err(StoreError::Invalid("invalid observation row count".into()));
    }
    let mut rows = Vec::with_capacity(observations.len());
    let mut actual_object_lists = BTreeSet::new();
    for observation in observations {
        let value = serde_json::to_value(observation.data())?;
        let encoded_size = serde_json::to_vec(&value)?.len() as u64;
        if encoded_size > MAX_OBSERVATION_ROW_BYTES {
            return Err(StoreError::Invalid(
                "observation envelope exceeds row limit".into(),
            ));
        }
        let mut row = BTreeMap::new();
        flatten("", &value, &mut row, &mut actual_object_lists)?;
        rows.push(row);
    }
    let (logical_columns, object_lists) = fixed_contract()?;
    let mut actual_columns = LogicalColumns::new();
    collect_logical_columns(&rows, &mut actual_columns)?;
    for (path, kinds) in actual_columns {
        let contract_kinds = logical_columns.get(&path).ok_or_else(|| {
            StoreError::Corrupt(format!(
                "observation field path {path} is outside fixed V2 schema"
            ))
        })?;
        if !kinds.is_subset(contract_kinds) {
            return Err(StoreError::Corrupt(format!(
                "observation field path {path} has a type outside fixed V2 schema"
            )));
        }
    }
    if !actual_object_lists.is_subset(&object_lists) {
        return Err(StoreError::Corrupt(
            "observation object-list path is outside fixed V2 schema".into(),
        ));
    }
    let (union_paths, columns) = physical_columns(&logical_columns)?;
    let fields = columns
        .iter()
        .map(|(physical_path, (_, column_kind))| {
            Field::new(physical_path, data_type_for_column_kind(*column_kind), true)
        })
        .collect::<Vec<_>>();
    let metadata_value = format!(
        "{METADATA_PREFIX}{};union-paths={}",
        object_lists.iter().cloned().collect::<Vec<_>>().join(","),
        union_paths.iter().cloned().collect::<Vec<_>>().join(",")
    );
    let mut metadata = std::collections::HashMap::new();
    metadata.insert(METADATA_KEY.to_owned(), metadata_value);
    let schema = Arc::new(Schema::new_with_metadata(fields, metadata));
    let arrays = columns
        .iter()
        .map(|(_, (logical_path, column_kind))| build_array(logical_path, *column_kind, &rows))
        .collect::<Result<Vec<_>>>()?;
    let batch = RecordBatch::try_new(schema.clone(), arrays).map_err(codec_error)?;
    let properties = WriterProperties::builder()
        .set_created_by("kyberia-project-store".to_owned())
        .set_compression(Compression::UNCOMPRESSED)
        .set_dictionary_enabled(false)
        .set_max_row_group_row_count(Some(MAX_BATCH_SIZE))
        .build();
    let mut sink = BoundedWriter::default();
    {
        let mut writer =
            ArrowWriter::try_new(&mut sink, schema, Some(properties)).map_err(codec_error)?;
        writer.write(&batch).map_err(codec_error)?;
        writer.close().map_err(codec_error)?;
    }
    if sink.bytes.is_empty() || sink.bytes.len() as u64 > MAX_OBSERVATION_CHUNK_BYTES {
        return Err(StoreError::Invalid("invalid Parquet output size".into()));
    }
    Ok(sink.bytes)
}

fn data_type_for_column_kind(column_kind: ColumnKind) -> DataType {
    match column_kind {
        ColumnKind::Scalar(kind) => kind.data_type(),
        ColumnKind::List(kind) => {
            DataType::List(Arc::new(Field::new("item", kind.data_type(), true)))
        }
    }
}

fn validate_contract_fields(
    schema: &Schema,
    contract_columns: &PhysicalColumns,
) -> Result<Vec<(String, ColumnKind)>> {
    if schema.fields().len() != contract_columns.len() {
        return Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION));
    }
    let mut columns = Vec::with_capacity(schema.fields().len());
    let mut names = BTreeSet::new();
    for field in schema.fields() {
        if !valid_path(field.name()) || !names.insert(field.name().to_owned()) {
            return Err(StoreError::Corrupt(
                "duplicate or empty Parquet field name".into(),
            ));
        }
        let expected = contract_columns
            .get(field.name())
            .ok_or_else(|| StoreError::UnsupportedChunkVersion(FORMAT_VERSION))?;
        // Compare the complete Arrow Field contract. This includes the
        // top-level nullability and, recursively, a List child's name,
        // datatype, nullability, and metadata.
        let expected_field = Field::new(
            field.name().clone(),
            data_type_for_column_kind(expected.1),
            true,
        );
        if field.as_ref() != &expected_field {
            return Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION));
        }
        columns.push(expected.clone());
    }
    if names.len() != contract_columns.len()
        || contract_columns
            .keys()
            .enumerate()
            .any(|(index, name)| schema.field(index).name() != name)
    {
        return Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION));
    }
    Ok(columns)
}

fn scalar_decoded_bytes(kind: Kind, values: u64) -> Result<u64> {
    let width = match kind {
        Kind::Bool => 1,
        Kind::I64 | Kind::U64 | Kind::F64 => 8,
        // Include one maximum-size string payload and one Arrow offset per
        // value. This bounds dictionary/RLE expansion before decoding pages.
        Kind::String => (MAX_STRING_BYTES as u64)
            .checked_add(4)
            .ok_or_else(|| StoreError::Corrupt("Arrow string budget overflow".into()))?,
    };
    values
        .checked_mul(width)
        .and_then(|bytes| bytes.checked_add(values))
        .ok_or_else(|| StoreError::Corrupt("Arrow decoded-byte budget overflow".into()))
}

fn decoded_arrow_budget(
    metadata: &parquet::file::metadata::ParquetMetaData,
    schema: &Schema,
    row_count: u64,
) -> Result<()> {
    let mut total = 0_u64;
    for row_group in metadata.row_groups() {
        let rows = u64::try_from(row_group.num_rows())
            .map_err(|_| StoreError::Corrupt("negative Parquet row-group count".into()))?;
        for (field, column) in schema.fields().iter().zip(row_group.columns()) {
            let values = u64::try_from(column.num_values())
                .map_err(|_| StoreError::Corrupt("negative Parquet value count".into()))?;
            let validity = rows
                .checked_add(7)
                .map(|bytes| bytes / 8)
                .ok_or_else(|| StoreError::Corrupt("Arrow validity budget overflow".into()))?;
            let bytes = match field.data_type() {
                DataType::Boolean => scalar_decoded_bytes(Kind::Bool, values)?,
                DataType::Int64 => scalar_decoded_bytes(Kind::I64, values)?,
                DataType::UInt64 => scalar_decoded_bytes(Kind::U64, values)?,
                DataType::Float64 => scalar_decoded_bytes(Kind::F64, values)?,
                DataType::Utf8 => scalar_decoded_bytes(Kind::String, values)?,
                DataType::List(child) => {
                    let kind = match child.data_type() {
                        DataType::Boolean => Kind::Bool,
                        DataType::Int64 => Kind::I64,
                        DataType::UInt64 => Kind::U64,
                        DataType::Float64 => Kind::F64,
                        DataType::Utf8 => Kind::String,
                        _ => {
                            return Err(StoreError::Corrupt(
                                "unsupported Parquet list child type".into(),
                            ));
                        }
                    };
                    let list_offsets = rows
                        .checked_add(1)
                        .and_then(|offsets| offsets.checked_mul(4))
                        .ok_or_else(|| {
                            StoreError::Corrupt("Arrow list offset budget overflow".into())
                        })?;
                    let child_bytes = scalar_decoded_bytes(kind, values)?;
                    list_offsets
                        .checked_add(validity)
                        .and_then(|bytes| bytes.checked_add(child_bytes))
                        .ok_or_else(|| StoreError::Corrupt("Arrow list budget overflow".into()))?
                }
                _ => {
                    return Err(StoreError::Corrupt(
                        "unsupported Parquet column type".into(),
                    ));
                }
            };
            total = total
                .checked_add(bytes)
                .ok_or_else(|| StoreError::Corrupt("Arrow decoded-byte budget overflow".into()))?;
            if total > MAX_DECODED_ARROW_BYTES {
                return Err(StoreError::Corrupt(
                    "decoded Arrow allocation budget exceeded".into(),
                ));
            }
        }
    }
    // Keep the parameter explicit so the estimate remains tied to the
    // authoritative row count even when a future schema adds nested arrays.
    if row_count == 0 {
        return Err(StoreError::Corrupt("Parquet row count is zero".into()));
    }
    Ok(())
}

fn scalar_from_array(array: &dyn Array, index: usize, kind: Kind) -> Result<Option<Scalar>> {
    if array.is_null(index) {
        return Ok(None);
    }
    match kind {
        Kind::Bool => Ok(Some(Scalar::Bool(
            array
                .as_any()
                .downcast_ref::<BooleanArray>()
                .ok_or_else(|| codec_error("BooleanArray type mismatch"))?
                .value(index),
        ))),
        Kind::I64 => Ok(Some(Scalar::I64(
            array
                .as_any()
                .downcast_ref::<Int64Array>()
                .ok_or_else(|| codec_error("Int64Array type mismatch"))?
                .value(index),
        ))),
        Kind::U64 => Ok(Some(Scalar::U64(
            array
                .as_any()
                .downcast_ref::<UInt64Array>()
                .ok_or_else(|| codec_error("UInt64Array type mismatch"))?
                .value(index),
        ))),
        Kind::F64 => Ok(Some(Scalar::F64(
            array
                .as_any()
                .downcast_ref::<Float64Array>()
                .ok_or_else(|| codec_error("Float64Array type mismatch"))?
                .value(index),
        ))),
        Kind::String => {
            let value = array
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| codec_error("StringArray type mismatch"))?
                .value(index);
            if value.len() > MAX_STRING_BYTES {
                return Err(StoreError::Corrupt(
                    "Parquet string exceeds observation text limit".into(),
                ));
            }
            Ok(Some(Scalar::String(value.to_owned())))
        }
    }
}

fn value_from_scalar(value: Scalar) -> Result<Value> {
    match value {
        Scalar::Bool(value) => Ok(Value::Bool(value)),
        Scalar::I64(value) => Ok(Value::Number(value.into())),
        Scalar::U64(value) => Ok(Value::Number(value.into())),
        Scalar::F64(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| StoreError::Corrupt("non-finite Parquet scalar".into())),
        Scalar::String(value) => Ok(Value::String(value)),
    }
}

fn validate_parquet_metadata(
    metadata: &parquet::file::metadata::ParquetMetaData,
    schema: &Schema,
    file_len: usize,
    row_count: u64,
    column_count: usize,
) -> Result<()> {
    if metadata.num_row_groups() == 0 || metadata.num_row_groups() > MAX_ROW_GROUPS {
        return Err(StoreError::Corrupt(
            "Parquet row-group count is outside bounds".into(),
        ));
    }
    let mut total_rows = 0_u64;
    let mut total_compressed = 0_u64;
    let mut total_uncompressed = 0_u64;
    for row_group in metadata.row_groups() {
        let rows = u64::try_from(row_group.num_rows())
            .map_err(|_| StoreError::Corrupt("negative Parquet row-group count".into()))?;
        if rows == 0 || rows > MAX_OBSERVATION_CHUNK_ROWS {
            return Err(StoreError::Corrupt(
                "Parquet row-group rows are outside bounds".into(),
            ));
        }
        if row_group.num_columns() != column_count {
            return Err(StoreError::Corrupt(
                "Parquet row-group/schema column mismatch".into(),
            ));
        }
        total_rows = total_rows
            .checked_add(rows)
            .ok_or_else(|| StoreError::Corrupt("Parquet row count overflow".into()))?;
        if schema.fields().len() != column_count {
            return Err(StoreError::Corrupt(
                "Parquet schema/field count mismatch".into(),
            ));
        }
        for (field, column) in schema.fields().iter().zip(row_group.columns()) {
            if column.compression() != Compression::UNCOMPRESSED {
                return Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION));
            }
            let values = u64::try_from(column.num_values())
                .map_err(|_| StoreError::Corrupt("negative Parquet value count".into()))?;
            // Parquet `num_values` counts leaf values. Definition and
            // repetition levels do not add a separate parent value, so no
            // extra allowance is needed for nullable list containers.
            let max_values = match field.data_type() {
                DataType::List(_) => rows
                    .checked_mul(MAX_LIST_VALUES as u64)
                    .ok_or_else(|| StoreError::Corrupt("Parquet value count overflow".into()))?,
                _ => rows,
            };
            if values == 0 || values > max_values {
                return Err(StoreError::Corrupt(
                    "Parquet column values are outside bounds".into(),
                ));
            }
            let compressed = u64::try_from(column.compressed_size())
                .map_err(|_| StoreError::Corrupt("negative Parquet compressed size".into()))?;
            let uncompressed = u64::try_from(column.uncompressed_size())
                .map_err(|_| StoreError::Corrupt("negative Parquet uncompressed size".into()))?;
            if compressed == 0 || uncompressed == 0 || uncompressed > MAX_OBSERVATION_CHUNK_BYTES {
                return Err(StoreError::Corrupt(
                    "Parquet column byte sizes are outside bounds".into(),
                ));
            }
            total_compressed = total_compressed
                .checked_add(compressed)
                .ok_or_else(|| StoreError::Corrupt("Parquet compressed size overflow".into()))?;
            total_uncompressed = total_uncompressed
                .checked_add(uncompressed)
                .ok_or_else(|| StoreError::Corrupt("Parquet uncompressed size overflow".into()))?;
            if total_compressed > file_len as u64
                || total_uncompressed > MAX_OBSERVATION_CHUNK_BYTES
            {
                return Err(StoreError::Corrupt(
                    "Parquet column byte budget exceeded".into(),
                ));
            }
            let start = column
                .dictionary_page_offset()
                .unwrap_or_else(|| column.data_page_offset());
            let start = u64::try_from(start)
                .map_err(|_| StoreError::Corrupt("negative Parquet page offset".into()))?;
            let end = start
                .checked_add(compressed)
                .ok_or_else(|| StoreError::Corrupt("Parquet page offset overflow".into()))?;
            if start < 4 || end > file_len as u64 {
                return Err(StoreError::Corrupt(
                    "Parquet column chunk lies outside artifact".into(),
                ));
            }
        }
    }
    if total_rows != row_count {
        return Err(StoreError::Corrupt(
            "Parquet row-group row count mismatch".into(),
        ));
    }
    Ok(())
}

fn list_values(array: &ListArray, index: usize, kind: Kind) -> Result<Option<Vec<Option<Scalar>>>> {
    if array.is_null(index) {
        return Ok(None);
    }
    let offsets = array.value_offsets();
    let start = usize::try_from(
        *offsets
            .get(index)
            .ok_or_else(|| StoreError::Corrupt("Parquet list offset is out of bounds".into()))?,
    )
    .map_err(|_| StoreError::Corrupt("negative Parquet list offset".into()))?;
    let end = usize::try_from(
        *offsets
            .get(index + 1)
            .ok_or_else(|| StoreError::Corrupt("Parquet list offset is out of bounds".into()))?,
    )
    .map_err(|_| StoreError::Corrupt("negative Parquet list offset".into()))?;
    if end < start || end > array.values().len() {
        return Err(StoreError::Corrupt("invalid Parquet list offsets".into()));
    }
    if end - start > MAX_LIST_VALUES {
        return Err(StoreError::Corrupt(
            "Parquet list exceeds observation resource limit".into(),
        ));
    }
    let values = array.value(index);
    let mut result = Vec::with_capacity(values.len());
    for index in 0..values.len() {
        result.push(scalar_from_array(values.as_ref(), index, kind)?);
    }
    Ok(Some(result))
}

fn merge_flat_values(existing: FlatValue, incoming: FlatValue) -> Result<FlatValue> {
    match (existing, incoming) {
        (FlatValue::Scalar(_), FlatValue::Scalar(_))
        | (FlatValue::Scalar(_), FlatValue::List(_))
        | (FlatValue::List(_), FlatValue::Scalar(_)) => Err(StoreError::Corrupt(
            "multiple Parquet union values in one row".into(),
        )),
        (FlatValue::List(existing), FlatValue::List(incoming)) => {
            if existing.len() != incoming.len() {
                return Err(StoreError::Corrupt(
                    "Parquet union list lengths differ".into(),
                ));
            }
            let mut merged = Vec::with_capacity(existing.len());
            for (existing, incoming) in existing.into_iter().zip(incoming) {
                match (existing, incoming) {
                    (Some(_), Some(_)) => {
                        return Err(StoreError::Corrupt(
                            "multiple Parquet union values in one list element".into(),
                        ));
                    }
                    (Some(value), None) | (None, Some(value)) => merged.push(Some(value)),
                    (None, None) => merged.push(None),
                }
            }
            Ok(FlatValue::List(merged))
        }
    }
}

fn insert_flat_value(
    values: &mut BTreeMap<String, FlatValue>,
    path: &str,
    incoming: FlatValue,
) -> Result<()> {
    if let Some(existing) = values.remove(path) {
        values.insert(path.to_owned(), merge_flat_values(existing, incoming)?);
    } else {
        values.insert(path.to_owned(), incoming);
    }
    Ok(())
}

fn set_path(root: &mut Map<String, Value>, path: &str, value: Value) -> Result<()> {
    let parts = path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(StoreError::Corrupt(
            "empty observation Parquet field name".into(),
        ));
    }
    let mut object = root;
    for part in &parts[..parts.len() - 1] {
        let entry = object
            .entry((*part).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        object = entry
            .as_object_mut()
            .ok_or_else(|| StoreError::Corrupt("Parquet path conflicts with scalar".into()))?;
    }
    object.insert(parts[parts.len() - 1].to_owned(), value);
    Ok(())
}

pub fn decode(bytes: &[u8]) -> Result<Vec<kyberia_domain::observation::ObservationEnvelope>> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_OBSERVATION_CHUNK_BYTES {
        return Err(StoreError::Corrupt(
            "observation Parquet exceeds read budget".into(),
        ));
    }
    if bytes.get(..4) != Some(b"PAR1")
        || bytes.get(bytes.len().saturating_sub(4)..) != Some(b"PAR1")
    {
        return Err(StoreError::Corrupt("invalid Parquet magic".into()));
    }
    let builder = ParquetRecordBatchReaderBuilder::try_new(Bytes::copy_from_slice(bytes))
        .map_err(codec_error)?;
    let metadata = builder.metadata();
    let file_metadata = metadata.file_metadata();
    let row_count = u64::try_from(file_metadata.num_rows())
        .map_err(|_| StoreError::Corrupt("Parquet row count overflow".into()))?;
    if row_count == 0 || row_count > MAX_OBSERVATION_CHUNK_ROWS {
        return Err(StoreError::Corrupt(
            "Parquet row count is outside bounds".into(),
        ));
    }
    let schema = builder.schema();
    if schema.fields().is_empty() || schema.fields().len() > MAX_COLUMNS {
        return Err(StoreError::Corrupt(
            "Parquet column count is outside bounds".into(),
        ));
    }
    validate_parquet_metadata(
        metadata,
        schema,
        bytes.len(),
        row_count,
        schema.fields().len(),
    )?;
    let metadata_value = schema
        .metadata
        .get(METADATA_KEY)
        .ok_or_else(|| StoreError::Corrupt("missing Kyberia Parquet schema metadata".into()))?;
    let metadata_value = metadata_value
        .strip_prefix(METADATA_PREFIX)
        .ok_or_else(|| StoreError::UnsupportedChunkVersion(FORMAT_VERSION))?;
    let (object_lists, union_paths) = metadata_value
        .split_once(";union-paths=")
        .ok_or_else(|| StoreError::Corrupt("malformed Kyberia Parquet schema metadata".into()))?;
    let object_lists = object_lists
        .split(',')
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    if object_lists.iter().any(|path| !valid_path(path)) {
        return Err(StoreError::Corrupt(
            "invalid Parquet object-list path".into(),
        ));
    }
    let union_paths = union_paths
        .split(',')
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    if union_paths.iter().any(|path| !valid_path(path)) {
        return Err(StoreError::Corrupt("invalid Parquet union path".into()));
    }
    let (contract_logical_columns, contract_object_lists) = fixed_contract()?;
    let (contract_union_paths, contract_columns) = physical_columns(&contract_logical_columns)?;
    if object_lists != contract_object_lists || union_paths != contract_union_paths {
        return Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION));
    }
    let columns = validate_contract_fields(schema, &contract_columns)?;
    decoded_arrow_budget(metadata, schema, row_count)?;
    let reader = builder
        .with_batch_size(MAX_BATCH_SIZE)
        .build()
        .map_err(codec_error)?;
    let mut result = Vec::with_capacity(row_count as usize);
    for batch in reader {
        let batch = batch.map_err(codec_error)?;
        if batch.num_columns() != columns.len() {
            return Err(StoreError::Corrupt("Parquet batch/schema mismatch".into()));
        }
        for row_index in 0..batch.num_rows() {
            let mut values = BTreeMap::new();
            for (column_index, (path, column_kind)) in columns.iter().enumerate() {
                let array = batch.column(column_index);
                match column_kind {
                    ColumnKind::Scalar(kind) => {
                        if let Some(value) = scalar_from_array(array.as_ref(), row_index, *kind)? {
                            insert_flat_value(&mut values, path, FlatValue::Scalar(value))?;
                        }
                    }
                    ColumnKind::List(kind) => {
                        let array = array
                            .as_any()
                            .downcast_ref::<ListArray>()
                            .ok_or_else(|| codec_error("ListArray type mismatch"))?;
                        if let Some(list) = list_values(array, row_index, *kind)? {
                            insert_flat_value(&mut values, path, FlatValue::List(list))?;
                        }
                    }
                }
            }
            let mut root = Map::new();
            let mut grouped = BTreeSet::new();
            for object_list in &object_lists {
                let prefix = format!("{object_list}.");
                let members = columns
                    .iter()
                    .map(|(path, _)| path)
                    .filter(|path| path.starts_with(&prefix))
                    .cloned()
                    .collect::<BTreeSet<_>>();
                if members.is_empty() {
                    set_path(&mut root, object_list, Value::Array(Vec::new()))?;
                    continue;
                }
                let length = members
                    .iter()
                    .filter_map(|path| values.get(path))
                    .find_map(|value| match value {
                        FlatValue::List(values) => Some(values.len()),
                        FlatValue::Scalar(_) => None,
                    })
                    .unwrap_or(0);
                let mut objects = vec![Map::new(); length];
                for path in members {
                    let list = match values.get(&path) {
                        Some(FlatValue::List(values)) => values,
                        None if length == 0 => continue,
                        _ => {
                            return Err(StoreError::Corrupt(
                                "invalid Parquet object-list field".into(),
                            ));
                        }
                    };
                    if list.len() != length {
                        return Err(StoreError::Corrupt(
                            "Parquet object-list lengths differ".into(),
                        ));
                    }
                    let suffix = path.strip_prefix(&prefix).expect("prefix checked");
                    for (index, value) in list.iter().cloned().enumerate() {
                        let value = value.ok_or_else(|| {
                            StoreError::Corrupt(
                                "null element in observation Parquet object list".into(),
                            )
                        })?;
                        set_path(&mut objects[index], suffix, value_from_scalar(value)?)?;
                    }
                    grouped.insert(path);
                }
                set_path(
                    &mut root,
                    object_list,
                    Value::Array(objects.into_iter().map(Value::Object).collect()),
                )?;
            }
            for (path, value) in values {
                if grouped.contains(&path) {
                    continue;
                }
                match value {
                    FlatValue::Scalar(value) => {
                        set_path(&mut root, &path, value_from_scalar(value)?)?
                    }
                    FlatValue::List(values) => set_path(
                        &mut root,
                        &path,
                        Value::Array(
                            values
                                .into_iter()
                                .map(|value| {
                                    value
                                        .ok_or_else(|| {
                                            StoreError::Corrupt(
                                                "null element in observation Parquet list".into(),
                                            )
                                        })
                                        .and_then(value_from_scalar)
                                })
                                .collect::<Result<Vec<_>>>()?,
                        ),
                    )?,
                }
            }
            let envelope_value = Value::Object(root);
            if serde_json::to_vec(&envelope_value)?.len() as u64 > MAX_OBSERVATION_ROW_BYTES {
                return Err(StoreError::Corrupt(
                    "reconstructed observation exceeds row limit".into(),
                ));
            }
            let data: kyberia_domain::observation::EnvelopeData =
                serde_json::from_value(envelope_value).map_err(|_| {
                    StoreError::Corrupt("malformed observation Parquet envelope".into())
                })?;
            let observation = kyberia_domain::observation::ObservationEnvelope::new(data)
                .map_err(|_| StoreError::Corrupt("invalid observation Parquet envelope".into()))?;
            result.push(observation);
            if result.len() as u64 > MAX_OBSERVATION_CHUNK_ROWS {
                return Err(StoreError::Corrupt("Parquet row limit exceeded".into()));
            }
        }
    }
    if result.len() as u64 != row_count {
        return Err(StoreError::Corrupt("Parquet row count mismatch".into()));
    }
    for pair in result.windows(2) {
        if pair[0].data().id >= pair[1].data().id {
            return Err(StoreError::Corrupt(
                "observation rows are not in canonical ID order".into(),
            ));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::{
        basic::{Compression, Repetition, Type as PhysicalType},
        file::metadata::{ColumnChunkMetaData, FileMetaData, ParquetMetaData, RowGroupMetaData},
        schema::types::{SchemaDescriptor, Type},
    };

    fn metadata_fixture(
        compression: Compression,
        num_values: i64,
        uncompressed_size: i64,
    ) -> ParquetMetaData {
        metadata_fixture_with_type(
            PhysicalType::INT64,
            compression,
            num_values,
            uncompressed_size,
        )
    }

    fn metadata_fixture_with_type(
        physical_type: PhysicalType,
        compression: Compression,
        num_values: i64,
        uncompressed_size: i64,
    ) -> ParquetMetaData {
        let field = Type::primitive_type_builder("value", physical_type)
            .with_repetition(Repetition::OPTIONAL)
            .build()
            .unwrap();
        let root = Type::group_type_builder("schema")
            .with_fields(vec![Arc::new(field)])
            .build()
            .unwrap();
        let schema = Arc::new(SchemaDescriptor::new(Arc::new(root)));
        let column = ColumnChunkMetaData::builder(schema.column(0))
            .set_num_values(num_values)
            .set_compression(compression)
            .set_total_compressed_size(1)
            .set_total_uncompressed_size(uncompressed_size)
            .set_data_page_offset(4)
            .build()
            .unwrap();
        let row_group = RowGroupMetaData::builder(Arc::clone(&schema))
            .set_num_rows(1)
            .set_column_metadata(vec![column])
            .build()
            .unwrap();
        let file = FileMetaData::new(1, 1, None, None, schema, None);
        ParquetMetaData::new(file, vec![row_group])
    }

    #[test]
    fn predecode_budget_rejects_declared_compression() {
        let metadata = metadata_fixture(Compression::GZIP(Default::default()), 1, 1);
        assert!(matches!(
            validate_parquet_metadata(
                &metadata,
                &Schema::new(vec![Arc::new(Field::new("value", DataType::Int64, true))]),
                1024,
                1,
                1,
            ),
            Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION))
        ));
    }

    #[test]
    fn predecode_budget_rejects_declared_oversized_column() {
        let metadata = metadata_fixture(
            Compression::UNCOMPRESSED,
            1,
            (MAX_OBSERVATION_CHUNK_BYTES + 1) as i64,
        );
        assert!(matches!(
            validate_parquet_metadata(
                &metadata,
                &Schema::new(vec![Arc::new(Field::new("value", DataType::Int64, true))]),
                1024,
                1,
                1,
            ),
            Err(StoreError::Corrupt(message)) if message.contains("byte sizes")
        ));
    }

    #[test]
    fn predecode_budget_rejects_excess_declared_values() {
        let metadata = metadata_fixture(Compression::UNCOMPRESSED, 1026, 1);
        assert!(matches!(
            validate_parquet_metadata(
                &metadata,
                &Schema::new(vec![Arc::new(Field::new("value", DataType::Int64, true))]),
                1024,
                1,
                1,
            ),
            Err(StoreError::Corrupt(message)) if message.contains("column values")
        ));
    }

    #[test]
    fn predecode_value_bound_is_type_aware_and_admits_list_limit() {
        let list_schema = Schema::new(vec![Arc::new(Field::new(
            "value",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        ))]);
        let at_limit = metadata_fixture(Compression::UNCOMPRESSED, MAX_LIST_VALUES as i64, 1);
        assert!(validate_parquet_metadata(&at_limit, &list_schema, 1024, 1, 1).is_ok());

        let over_limit =
            metadata_fixture(Compression::UNCOMPRESSED, (MAX_LIST_VALUES + 1) as i64, 1);
        assert!(matches!(
            validate_parquet_metadata(&over_limit, &list_schema, 1024, 1, 1),
            Err(StoreError::Corrupt(message)) if message.contains("column values")
        ));

        let scalar_schema = Schema::new(vec![Arc::new(Field::new("value", DataType::Int64, true))]);
        let scalar_over_limit = metadata_fixture(Compression::UNCOMPRESSED, 2, 1);
        assert!(matches!(
            validate_parquet_metadata(&scalar_over_limit, &scalar_schema, 1024, 1, 1),
            Err(StoreError::Corrupt(message)) if message.contains("column values")
        ));
    }

    #[test]
    fn nested_list_field_contract_includes_child_name_and_nullability() {
        let expected = Field::new(
            "values",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        );
        let mut contract = PhysicalColumns::new();
        contract.insert(
            "values".into(),
            ("values".into(), ColumnKind::List(Kind::String)),
        );
        assert!(
            validate_contract_fields(&Schema::new(vec![Arc::new(expected.clone())]), &contract)
                .is_ok()
        );

        let altered_name = Field::new(
            "values",
            DataType::List(Arc::new(Field::new("wrong", DataType::Utf8, true))),
            true,
        );
        assert!(matches!(
            validate_contract_fields(&Schema::new(vec![Arc::new(altered_name)]), &contract),
            Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION))
        ));

        let altered_nullability = Field::new(
            "values",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, false))),
            true,
        );
        assert!(matches!(
            validate_contract_fields(&Schema::new(vec![Arc::new(altered_nullability)]), &contract),
            Err(StoreError::UnsupportedChunkVersion(FORMAT_VERSION))
        ));
    }

    #[test]
    fn decoded_budget_rejects_repeated_string_expansion_before_reader_build() {
        let metadata = metadata_fixture_with_type(
            PhysicalType::BYTE_ARRAY,
            Compression::UNCOMPRESSED,
            65_536,
            1,
        );
        let schema = Schema::new(vec![Arc::new(Field::new("value", DataType::Utf8, true))]);
        assert!(matches!(
            decoded_arrow_budget(&metadata, &schema, 65_536),
            Err(StoreError::Corrupt(message))
                if message.contains("decoded Arrow allocation budget")
        ));
    }
}
