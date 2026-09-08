//! Defensive boundary for the metadata tables in a project bundle.
//! Authorizers constrain SQL operations; they are not an OS or hard-memory sandbox.
use crate::{Result, StoreError, manifest::MAX_MANIFEST_BYTES};
use rusqlite::{
    Connection,
    config::DbConfig,
    hooks::{AuthAction, AuthContext, Authorization},
    limits::Limit,
};
use std::time::{Duration, Instant};

pub(crate) const CREATE_MANIFEST: &str = "CREATE TABLE bundle_manifest (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL CHECK(revision>=0), body BLOB NOT NULL)";
pub(crate) const CREATE_SURVEY_SNAPSHOTS: &str = "CREATE TABLE survey_snapshots (snapshot_id TEXT PRIMARY KEY CHECK(length(snapshot_id)=32), project_id TEXT NOT NULL CHECK(length(project_id)=32), session_id TEXT NOT NULL CHECK(length(session_id)=32), point_id TEXT NOT NULL CHECK(length(point_id)=32), source_id TEXT NOT NULL CHECK(length(source_id)=32), collector_id TEXT NOT NULL CHECK(length(collector_id)=32), artifact_hash TEXT NOT NULL CHECK(length(artifact_hash)=64), input_schema TEXT NOT NULL, output_schema TEXT NOT NULL, decoder_version TEXT NOT NULL, source_version TEXT NOT NULL, created_utc_ms INTEGER NOT NULL CHECK(created_utc_ms>=0), revision INTEGER NOT NULL CHECK(revision>=0))";
pub(crate) const CREATE_SURVEY_SNAPSHOT_HISTORY: &str = "CREATE TABLE survey_snapshot_history (revision INTEGER PRIMARY KEY CHECK(revision>=0), snapshot_id TEXT NOT NULL CHECK(length(snapshot_id)=32), project_id TEXT NOT NULL CHECK(length(project_id)=32), session_id TEXT NOT NULL CHECK(length(session_id)=32), point_id TEXT NOT NULL CHECK(length(point_id)=32), source_id TEXT NOT NULL CHECK(length(source_id)=32), collector_id TEXT NOT NULL CHECK(length(collector_id)=32), artifact_hash TEXT NOT NULL CHECK(length(artifact_hash)=64), input_schema TEXT NOT NULL, output_schema TEXT NOT NULL, decoder_version TEXT NOT NULL, source_version TEXT NOT NULL, operation TEXT NOT NULL, committed_utc_ms INTEGER NOT NULL CHECK(committed_utc_ms>=0))";
pub(crate) const CREATE_OPERATION_LOG_STATE: &str = "CREATE TABLE operation_log_state (singleton INTEGER PRIMARY KEY CHECK(singleton=1), project_id TEXT NOT NULL CHECK(length(project_id)=32), project_revision INTEGER NOT NULL CHECK(project_revision>=0))";
pub(crate) const CREATE_OPERATIONS: &str = "CREATE TABLE project_operations (operation_id TEXT PRIMARY KEY CHECK(length(operation_id)=32), project_id TEXT NOT NULL CHECK(length(project_id)=32), project_revision INTEGER NOT NULL CHECK(project_revision>0), logical_time INTEGER NOT NULL CHECK(logical_time>0), causal_depth INTEGER NOT NULL CHECK(causal_depth>=0), content_hash TEXT NOT NULL CHECK(length(content_hash)=64), canonical_bytes BLOB NOT NULL CHECK(length(canonical_bytes)>0 AND length(canonical_bytes)<=32768), wire_bytes BLOB NOT NULL CHECK(length(wire_bytes)>0 AND length(wire_bytes)<=49152))";
pub(crate) const CREATE_OBSERVATION_CHUNKS: &str = "CREATE TABLE observation_chunks (chunk_hash TEXT PRIMARY KEY CHECK(length(chunk_hash)=64), bytes INTEGER NOT NULL CHECK(bytes>0), media_type TEXT NOT NULL, schema_version INTEGER NOT NULL CHECK(schema_version>0), codec_version INTEGER NOT NULL CHECK(codec_version>0), row_count INTEGER NOT NULL CHECK(row_count>0), first_observation_id TEXT NOT NULL CHECK(length(first_observation_id)=32), last_observation_id TEXT NOT NULL CHECK(length(last_observation_id)=32), known_utc_count INTEGER NOT NULL CHECK(known_utc_count>=0 AND known_utc_count<=row_count), first_utc_ns INTEGER, last_utc_ns INTEGER, first_source_id TEXT NOT NULL CHECK(length(first_source_id)=32), last_source_id TEXT NOT NULL CHECK(length(last_source_id)=32), first_session_id TEXT NOT NULL CHECK(length(first_session_id)=32), last_session_id TEXT NOT NULL CHECK(length(last_session_id)=32), provenance_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), CHECK((known_utc_count=0 AND first_utc_ns IS NULL AND last_utc_ns IS NULL) OR (known_utc_count>0 AND first_utc_ns IS NOT NULL AND last_utc_ns IS NOT NULL AND first_utc_ns<=last_utc_ns)))";
pub(crate) const CREATE_OBSERVATION_CHUNK_MEMBERS: &str = "CREATE TABLE observation_chunk_members (chunk_hash TEXT NOT NULL CHECK(length(chunk_hash)=64), observation_id TEXT PRIMARY KEY CHECK(length(observation_id)=32), source_id TEXT NOT NULL CHECK(length(source_id)=32), session_id TEXT NOT NULL CHECK(length(session_id)=32), ordinal INTEGER NOT NULL CHECK(ordinal>=0), FOREIGN KEY(chunk_hash) REFERENCES observation_chunks(chunk_hash))";
pub(crate) const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SCHEMA_OBJECTS: usize = 64;
const MAX_VM_OPERATIONS: u64 = 2_000_000;
const SQL_DEADLINE: Duration = Duration::from_secs(5);

/// Called before the first SQL statement, including schema inspection.
pub(crate) fn initialize(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_secs(3))?;
    for (option, value) in [
        (DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true),
        (DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true),
    ] {
        connection.set_db_config(option, value)?;
    }
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, MAX_MANIFEST_BYTES as i32),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, 16 * 1024),
        (Limit::SQLITE_LIMIT_COLUMN, 32),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 32),
        (Limit::SQLITE_LIMIT_VDBE_OP, 100_000),
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
        (Limit::SQLITE_LIMIT_TRIGGER_DEPTH, 0),
    ] {
        connection.set_limit(limit, value)?;
    }
    start_operation(connection)
}

/// Reset only at a public operation boundary, never between its queries.
/// The callback bounds VM work and checks elapsed time every 1,000 operations.
/// Filesystem calls and SQLite mutex/I/O stalls are not preempted by this hook.
pub(crate) fn start_operation(connection: &Connection) -> Result<()> {
    let deadline = Instant::now() + SQL_DEADLINE;
    let mut operations = 0;
    connection.progress_handler(
        1000,
        Some(move || {
            operations += 1000;
            operations > MAX_VM_OPERATIONS || Instant::now() >= deadline
        }),
    )?;
    Ok(())
}

pub(crate) fn restrict(connection: &Connection, writable: bool) -> Result<()> {
    connection.authorizer(Some(move |context: AuthContext<'_>| {
        let allowed = context.accessor.is_none()
            && match context.action {
                AuthAction::Select | AuthAction::Transaction { .. } => true,
                AuthAction::Function {
                    function_name: "length",
                } => true,
                AuthAction::Read { table_name, .. } => {
                    context.database_name == Some("main")
                        && [
                            "sqlite_master",
                            "sqlite_schema",
                            "bundle_manifest",
                            "survey_snapshots",
                            "survey_snapshot_history",
                            "operation_log_state",
                            "project_operations",
                            "observation_chunks",
                            "observation_chunk_members",
                        ]
                        .contains(&table_name)
                }
                AuthAction::Insert {
                    table_name:
                        "survey_snapshots"
                        | "survey_snapshot_history"
                        | "operation_log_state"
                        | "project_operations"
                        | "observation_chunks"
                        | "observation_chunk_members",
                } => writable,
                AuthAction::Update {
                    table_name: "operation_log_state",
                    column_name: "project_revision",
                } => writable && context.database_name == Some("main"),
                AuthAction::Update {
                    table_name: "bundle_manifest",
                    column_name: "revision" | "body",
                } => writable && context.database_name == Some("main"),
                AuthAction::Pragma {
                    pragma_name: "user_version",
                    pragma_value: None,
                } => true,
                AuthAction::Pragma {
                    pragma_name: "quick_check",
                    pragma_value: Some("1"),
                } => true,
                AuthAction::Pragma {
                    pragma_name: "synchronous",
                    pragma_value: Some("FULL"),
                }
                | AuthAction::Pragma {
                    pragma_name: "journal_mode",
                    pragma_value: Some("DELETE"),
                } => writable,
                _ => false,
            };
        if allowed {
            Authorization::Allow
        } else {
            Authorization::Deny
        }
    }))?;
    Ok(())
}

/// Read SQLite's built-in schema catalog only. No imported table/view/trigger is
/// evaluated while establishing trust. Exact DDL admits the formats Kyberia has
/// actually written; an older manifest-only bundle remains readable and is
/// upgraded by the writable open path before snapshot operations are attempted.
pub(crate) fn validate_schema(connection: &Connection) -> Result<()> {
    // A canonical bundle has at most seven user tables and their SQLite-owned
    // autoindexes. Read a bounded inventory so malformed schema input cannot
    // allocate from an unbounded sqlite_schema result.
    let mut statement = connection
        .prepare("SELECT type,name,tbl_name,sql FROM main.sqlite_schema ORDER BY name LIMIT 65")?;
    let mut rows = statement.query([])?;
    let mut entries = Vec::new();
    let mut object_count = 0;
    while let Some(row) = rows.next()? {
        object_count += 1;
        if object_count > MAX_SCHEMA_OBJECTS {
            return Err(StoreError::Corrupt(
                "metadata schema inventory exceeds resource limit".into(),
            ));
        }
        let kind: String = row.get(0)?;
        let name: String = row.get(1)?;
        let table: String = row.get(2)?;
        let sql: Option<String> = row.get(3)?;
        // PRIMARY KEY declarations create SQLite-owned autoindexes. They are
        // implementation details of the admitted table shape and have no
        // executable SQL body; user-created indexes remain rejected below.
        if kind == "index" && name.starts_with("sqlite_autoindex_") && sql.is_none() {
            continue;
        }
        entries.push((kind, name, table, sql));
    }
    let expected_manifest = (
        "table",
        "bundle_manifest",
        "bundle_manifest",
        Some(CREATE_MANIFEST),
    );
    let expected_snapshots = (
        "table",
        "survey_snapshots",
        "survey_snapshots",
        Some(CREATE_SURVEY_SNAPSHOTS),
    );
    let expected_history = (
        "table",
        "survey_snapshot_history",
        "survey_snapshot_history",
        Some(CREATE_SURVEY_SNAPSHOT_HISTORY),
    );
    let expected_operation_state = (
        "table",
        "operation_log_state",
        "operation_log_state",
        Some(CREATE_OPERATION_LOG_STATE),
    );
    let expected_operations = (
        "table",
        "project_operations",
        "project_operations",
        Some(CREATE_OPERATIONS),
    );
    let expected_chunks = (
        "table",
        "observation_chunks",
        "observation_chunks",
        Some(CREATE_OBSERVATION_CHUNKS),
    );
    let expected_members = (
        "table",
        "observation_chunk_members",
        "observation_chunk_members",
        Some(CREATE_OBSERVATION_CHUNK_MEMBERS),
    );
    let manifest_valid = entries.iter().any(|entry| {
        entry.0 == expected_manifest.0
            && entry.1 == expected_manifest.1
            && entry.2 == expected_manifest.2
            && entry.3.as_deref() == expected_manifest.3
    });
    let optional_groups = [
        [expected_snapshots, expected_history],
        [expected_operation_state, expected_operations],
        [expected_chunks, expected_members],
    ];
    let mut known_objects = usize::from(manifest_valid);
    let groups_valid = optional_groups.iter().all(|group| {
        let present = entries
            .iter()
            .filter(|entry| group.iter().any(|candidate| entry.1 == candidate.1))
            .count();
        if present == 0 {
            return true;
        }
        if present != group.len()
            || !group.iter().all(|candidate| {
                entries.iter().any(|entry| {
                    entry.0 == candidate.0
                        && entry.1 == candidate.1
                        && entry.2 == candidate.2
                        && entry.3.as_deref() == candidate.3
                })
            })
        {
            return false;
        }
        known_objects += present;
        true
    });
    // Optional table groups are validated independently. This admits every
    // historical additive combination while rejecting partial groups and all
    // unrecognized schema objects.
    if !manifest_valid || !groups_valid || entries.len() != known_objects {
        return Err(StoreError::Corrupt(
            "unsupported physical metadata schema; expected canonical manifest, survey, operation, or observation tables"
                .into(),
        ));
    }
    Ok(())
}

pub(crate) fn has_survey_snapshot_schema(connection: &Connection) -> Result<bool> {
    let mut statement = connection.prepare(
        "SELECT name FROM main.sqlite_schema WHERE type='table' AND name IN ('survey_snapshots','survey_snapshot_history')",
    )?;
    let mut rows = statement.query([])?;
    let mut names = [false; 2];
    while let Some(row) = rows.next()? {
        match row.get::<_, String>(0)?.as_str() {
            "survey_snapshots" => names[0] = true,
            "survey_snapshot_history" => names[1] = true,
            _ => {}
        }
    }
    Ok(names.into_iter().all(|present| present))
}

pub(crate) fn has_operation_schema(connection: &Connection) -> Result<bool> {
    let mut statement = connection.prepare(
        "SELECT name FROM main.sqlite_schema WHERE type='table' AND name IN ('operation_log_state','project_operations')",
    )?;
    let mut rows = statement.query([])?;
    let mut names = [false; 2];
    while let Some(row) = rows.next()? {
        match row.get::<_, String>(0)?.as_str() {
            "operation_log_state" => names[0] = true,
            "project_operations" => names[1] = true,
            _ => {}
        }
    }
    Ok(names.into_iter().all(|present| present))
}

pub(crate) fn has_observation_chunk_schema(connection: &Connection) -> Result<bool> {
    let mut statement = connection.prepare(
        "SELECT name FROM main.sqlite_schema WHERE type='table' AND name IN ('observation_chunks','observation_chunk_members')",
    )?;
    let mut rows = statement.query([])?;
    let mut names = [false; 2];
    while let Some(row) = rows.next()? {
        match row.get::<_, String>(0)?.as_str() {
            "observation_chunks" => names[0] = true,
            "observation_chunk_members" => names[1] = true,
            _ => {}
        }
    }
    Ok(names.into_iter().all(|present| present))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_budget_interrupts_expensive_work_and_resets_for_next_operation() {
        let connection = Connection::open_in_memory().unwrap();
        initialize(&connection).unwrap();
        // Deliberately test the independent progress backstop without the
        // authorizer, which would reject this recursive SQL before evaluation.
        let error = connection.query_row::<i64, _, _>(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000000) SELECT sum(x) FROM n", [], |r| r.get(0)).unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::OperationInterrupted)
        );
        start_operation(&connection).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT 7", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
    }

    #[test]
    fn authorizer_rejects_recursive_sql_and_unrelated_actions() {
        let connection = Connection::open_in_memory().unwrap();
        initialize(&connection).unwrap();
        connection.execute_batch(CREATE_MANIFEST).unwrap();
        restrict(&connection, true).unwrap();
        for sql in [
            "CREATE TABLE unexpected(x)",
            "ATTACH ':memory:' AS other",
            "PRAGMA writable_schema=ON",
            "PRAGMA user_version=99",
            "DELETE FROM bundle_manifest",
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<3) SELECT x FROM n",
        ] {
            assert!(connection.execute_batch(sql).is_err(), "authorized: {sql}");
        }
        validate_schema(&connection).unwrap();
    }
}
