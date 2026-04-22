use std::path::Path;

use rusqlite::Connection;

use crate::domain::shared::AppError;

/// Registry of canonical migrations, embedded at compile time so a binary
/// shipped without the repository can still bring up a fresh database.
///
/// The tuple layout is `(version, sql)`. Versions must be strictly ascending
/// and unique — `run_migrations` asserts that invariant at runtime.
pub const MIGRATIONS: &[(u32, &str)] = &[(1, include_str!("../../../migrations/0001_init.sql"))];

/// Thin wrapper over a [`rusqlite::Connection`]. Exposes only the operations
/// application services need, so the rest of the codebase never mixes raw
/// `rusqlite` calls with orchestration logic.
#[derive(Debug)]
pub struct DbHandle {
    conn: Connection,
}

impl DbHandle {
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// Open or create a SQLite database at `path`. Applies the PRAGMAs required
/// for Rustory's durability model (foreign keys on, WAL journal, normal
/// sync) before returning, and verifies the effective journal mode is
/// really `wal` so a silent fallback to e.g. `delete` on a read-only or
/// networked mount is caught at boot rather than at write time.
pub fn open_at(path: &Path) -> Result<DbHandle, AppError> {
    let conn = Connection::open(path).map_err(|err| map_open_error(&err, "sqlite_open"))?;
    apply_foreign_keys(&conn)?;
    apply_wal_enforced(&conn)?;
    apply_synchronous(&conn)?;
    Ok(DbHandle { conn })
}

/// Open a fresh in-memory database. Used exclusively by tests — two calls
/// return two disjoint databases. WAL is not applicable to `:memory:`
/// databases (they report journal mode `memory`), so it is deliberately
/// not enforced here.
pub fn open_in_memory() -> Result<DbHandle, AppError> {
    let conn = Connection::open_in_memory()
        .map_err(|err| map_open_error(&err, "sqlite_open_in_memory"))?;
    apply_foreign_keys(&conn)?;
    apply_synchronous(&conn)?;
    Ok(DbHandle { conn })
}

fn apply_foreign_keys(conn: &Connection) -> Result<(), AppError> {
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|err| map_pragma_error(&err, "foreign_keys"))
}

fn apply_synchronous(conn: &Connection) -> Result<(), AppError> {
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|err| map_pragma_error(&err, "synchronous"))
}

/// Ask SQLite for WAL and verify the engine actually honored the request.
///
/// `PRAGMA journal_mode=WAL` can silently fall back to another mode on
/// read-only filesystems, some networked mounts, or cloud-sync folders.
/// Verify the effective mode so the runbook's WAL durability guarantee is
/// not merely aspirational — boot fails closed if WAL is unavailable.
fn apply_wal_enforced(conn: &Connection) -> Result<(), AppError> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|err| map_pragma_error(&err, "journal_mode"))?;
    let effective_mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .map_err(|err| map_pragma_error(&err, "journal_mode_verify"))?;
    if !effective_mode.eq_ignore_ascii_case("wal") {
        return Err(AppError::local_storage_unavailable(
            "Rustory ne peut pas activer le journal WAL sur cet emplacement.",
            "Déplace le dossier de données sur un disque local en écriture puis relance.",
        )
        .with_details(serde_json::json!({
            "source": "sqlite_pragma",
            "pragma": "journal_mode",
            "effective": effective_mode,
        })));
    }
    Ok(())
}

fn map_open_error(_err: &rusqlite::Error, source: &'static str) -> AppError {
    // Intentionally drop the raw rusqlite message from the wire payload: it
    // may embed the absolute path or a locale-specific string. The stable
    // `source` marker is enough for support while staying PII-free, per the
    // diagnostics rules inherited from `ensure_dir_writable`.
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu ouvrir sa base locale.",
        "Vérifie les permissions d'écriture puis relance l'application.",
    )
    .with_details(serde_json::json!({ "source": source }))
}

fn map_pragma_error(_err: &rusqlite::Error, pragma: &'static str) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu initialiser sa base locale.",
        "Supprime manuellement le fichier de base local puis relance l'application.",
    )
    .with_details(serde_json::json!({ "source": "sqlite_pragma", "pragma": pragma }))
}

fn map_migration_error(_err: &rusqlite::Error, stage: &'static str, version: u32) -> AppError {
    AppError::local_storage_unavailable(
        "Rustory n'a pas pu préparer sa base locale.",
        "Relance Rustory ; si le problème persiste, réinstalle l'application.",
    )
    .with_details(serde_json::json!({
        "source": "sqlite_migration",
        "stage": stage,
        "version": version,
    }))
}

/// Apply every migration in [`MIGRATIONS`] whose version has not yet been
/// recorded in `schema_migrations`. Idempotent: calling twice is a no-op
/// after the first successful run.
///
/// Each migration is wrapped in a single `BEGIN IMMEDIATE` transaction that
/// covers both the "already applied?" check and the DDL + ledger INSERT, so
/// two processes racing to initialize the database can never both decide a
/// version is pending and apply it twice.
pub fn run_migrations(db: &mut DbHandle) -> Result<(), AppError> {
    // `schema_migrations` is created outside of any user migration so that
    // the ledger itself survives a partially-rolled-back attempt.
    db.conn()
        .execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations ( \
               version INTEGER PRIMARY KEY, \
               applied_at TEXT NOT NULL \
             )",
            [],
        )
        .map_err(|err| map_migration_error(&err, "ensure_ledger", 0))?;

    let mut prev: Option<u32> = None;
    for (version, sql) in MIGRATIONS {
        if let Some(previous) = prev {
            assert!(
                *version > previous,
                "migration versions must be strictly ascending"
            );
        }
        prev = Some(*version);

        // `BEGIN IMMEDIATE` acquires the write lock up-front: a second
        // process running the same routine will either block here until the
        // first commits or fail fast with SQLITE_BUSY, which keeps the
        // "already applied?" check consistent with the INSERT in the same
        // transaction. A plain `DEFERRED` transaction would allow two
        // readers to concurrently miss the row and both try to apply.
        let tx = db
            .conn_mut()
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|err| map_migration_error(&err, "begin_transaction", *version))?;

        let already_applied: bool = tx
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE version = ?1",
                [version],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if already_applied {
            // Release the write lock without side effects; nothing to do.
            tx.rollback()
                .map_err(|err| map_migration_error(&err, "rollback_noop", *version))?;
            continue;
        }

        tx.execute_batch(sql)
            .map_err(|err| map_migration_error(&err, "apply_sql", *version))?;
        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![version, now_iso()],
        )
        .map_err(|err| map_migration_error(&err, "record_ledger", *version))?;
        tx.commit()
            .map_err(|err| map_migration_error(&err, "commit", *version))?;
    }

    Ok(())
}

fn now_iso() -> String {
    use time::format_description::well_known::Iso8601;
    use time::OffsetDateTime;
    // The ledger does not need millisecond precision — any ISO-8601 UTC
    // representation is enough to tell apart two runs on the same day.
    OffsetDateTime::now_utc()
        .format(&Iso8601::DEFAULT)
        .unwrap_or_else(|_| String::from("unknown"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::AppErrorCode;
    use tempfile::TempDir;

    #[test]
    fn open_at_creates_schema_migrations_table() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("db.sqlite");
        let mut db = open_at(&path).expect("open");

        run_migrations(&mut db).expect("migrate");

        let count: u32 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 1",
                [],
                |row| row.get(0),
            )
            .expect("query ledger");
        assert_eq!(count, 1);
    }

    #[test]
    fn run_migrations_is_idempotent() {
        let mut db = open_in_memory().expect("open");
        run_migrations(&mut db).expect("first apply");
        run_migrations(&mut db).expect("second apply");

        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count ledger");
        assert_eq!(count, 1, "ledger must not double-record a migration");
    }

    #[test]
    fn open_in_memory_returns_isolated_handles() {
        let mut a = open_in_memory().expect("a");
        let mut b = open_in_memory().expect("b");
        run_migrations(&mut a).expect("migrate a");
        run_migrations(&mut b).expect("migrate b");

        a.conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES ('id-a', 'A', 1, '{}', '0000000000000000000000000000000000000000000000000000000000000000', '2026-04-22T00:00:00.000Z', '2026-04-22T00:00:00.000Z')",
                [],
            )
            .expect("insert into a");

        let count_in_b: u32 = b
            .conn()
            .query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))
            .expect("count stories in b");
        assert_eq!(count_in_b, 0, "in-memory dbs must be disjoint");
    }

    #[test]
    fn open_at_fails_with_localstorage_unavailable_on_invalid_path() {
        let tmp = TempDir::new().expect("tempdir");
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"not-a-dir").expect("seed blocker file");
        // Target a SQLite path beneath a regular file — impossible to open.
        let target = blocker.join("db.sqlite");

        let err = open_at(&target).expect_err("must fail");
        assert_eq!(err.code, AppErrorCode::LocalStorageUnavailable);
        let details = err.details.as_ref().expect("details populated");
        assert_eq!(details["source"], "sqlite_open");
        // PII guardrail: neither the raw path nor the OS error message
        // must leak across the boundary.
        assert!(details.get("path").is_none());
        assert!(details.get("cause").is_none());
    }

    #[test]
    fn stories_table_enforces_check_constraints() {
        let mut db = open_in_memory().expect("open");
        run_migrations(&mut db).expect("migrate");

        let err = db
            .conn()
            .execute(
                "INSERT INTO stories (id, title, schema_version, structure_json, content_checksum, created_at, updated_at) \
                 VALUES ('id-empty', '   ', 1, '{}', '0000000000000000000000000000000000000000000000000000000000000000', '2026-04-22T00:00:00.000Z', '2026-04-22T00:00:00.000Z')",
                [],
            )
            .expect_err("blank title must trip CHECK");
        let message = err.to_string().to_lowercase();
        assert!(
            message.contains("check"),
            "expected CHECK constraint failure, got: {message}"
        );
    }
}
