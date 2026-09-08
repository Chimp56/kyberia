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
        (Limit::SQLITE_LIMIT_COLUMN, 16),
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
                AuthAction::Read { table_name, .. } => {
                    context.database_name == Some("main")
                        && [
                            "sqlite_master",
                            "sqlite_schema",
                            "bundle_manifest",
                            "survey_snapshots",
                            "survey_snapshot_history",
                        ]
                        .contains(&table_name)
                }
                AuthAction::Insert {
                    table_name: "survey_snapshots" | "survey_snapshot_history",
                } => writable,
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
    // A canonical bundle has at most three user tables and their SQLite-owned
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
    let valid_manifest_only = entries.len() == 1
        && entries.iter().any(|entry| {
            entry.0 == expected_manifest.0
                && entry.1 == expected_manifest.1
                && entry.2 == expected_manifest.2
                && entry.3.as_deref() == expected_manifest.3
        });
    let valid_current = entries.len() == 3
        && [expected_manifest, expected_snapshots, expected_history]
            .into_iter()
            .all(|expected| {
                entries.iter().any(|entry| {
                    entry.0 == expected.0
                        && entry.1 == expected.1
                        && entry.2 == expected.2
                        && entry.3.as_deref() == expected.3
                })
            });
    if !valid_manifest_only && !valid_current {
        return Err(StoreError::Corrupt(
            "unsupported physical metadata schema; expected the canonical manifest and survey tables"
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
