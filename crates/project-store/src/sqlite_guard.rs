//! Defensive boundary for the existing single-table metadata format.
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
pub(crate) const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;
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
                        && ["sqlite_master", "sqlite_schema", "bundle_manifest"]
                            .contains(&table_name)
                }
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
/// evaluated while establishing trust. Exact DDL admits the format Kyberia has
/// actually written; future logical versions remain readable with this envelope.
pub(crate) fn validate_schema(connection: &Connection) -> Result<()> {
    let mut statement =
        connection.prepare("SELECT type,name,tbl_name,sql FROM main.sqlite_schema LIMIT 2")?;
    let mut rows = statement.query([])?;
    let row = rows
        .next()?
        .ok_or_else(|| StoreError::Corrupt("missing metadata schema".into()))?;
    let kind: String = row.get(0)?;
    let name: String = row.get(1)?;
    let table: String = row.get(2)?;
    let sql: String = row.get(3)?;
    if kind != "table"
        || name != "bundle_manifest"
        || table != "bundle_manifest"
        || sql != CREATE_MANIFEST
        || rows.next()?.is_some()
    {
        return Err(StoreError::Corrupt(
            "unsupported physical metadata schema; expected the canonical manifest table only"
                .into(),
        ));
    }
    Ok(())
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
