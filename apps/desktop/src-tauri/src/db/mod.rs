mod migrations;

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::error::AppError;

pub(crate) fn map_constraint_error(error: AppError, context: &str) -> AppError {
    let AppError::Database(rusqlite::Error::SqliteFailure(ref failure, _)) = error else {
        return error;
    };
    let message = if failure.extended_code == 1555 || failure.extended_code == 2067 {
        format!("{context} already exists")
    } else if failure.extended_code == 19
        || failure.extended_code == 275
        || failure.extended_code == 787
    {
        format!("{context} violates a database constraint")
    } else {
        return error;
    };
    AppError::InvalidInput(message)
}

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let mut connection = Connection::open(path)?;
        Self::configure(&mut connection, true)?;
        migrations::run(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn open_in_memory() -> Result<Self, AppError> {
        let mut connection = Connection::open_in_memory()?;
        Self::configure(&mut connection, false)?;
        migrations::run(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn configure(connection: &mut Connection, file_backed: bool) -> Result<(), AppError> {
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        if file_backed {
            connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        }
        Ok(())
    }

    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    ) -> Result<T, AppError> {
        operation(&self.connection.lock()).map_err(AppError::from)
    }

    pub fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, rusqlite::Error>,
    ) -> Result<T, AppError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction()?;
        let result = operation(&transaction)?;
        transaction.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::Database;

    #[test]
    fn migration_creates_core_tables_and_version() {
        let database = Database::open_in_memory().expect("database opens");
        let table_count: i64 = database
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('plugin_installations', 'permission_grants', 'jobs', 'job_steps')",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("query succeeds");
        let version: i64 = database
            .with_connection(|connection| {
                connection.query_row("PRAGMA user_version", [], |row| row.get(0))
            })
            .expect("version query succeeds");
        assert_eq!(table_count, 4);
        assert_eq!(version, 2);
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let database = Database::open_in_memory().expect("database opens");
        let enabled: i64 = database
            .with_connection(|connection| {
                connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            })
            .expect("pragma query succeeds");
        assert_eq!(enabled, 1);
    }

    #[test]
    fn git_migration_is_idempotent_and_has_relationship_foreign_keys() {
        let database = Database::open_in_memory().expect("database opens");
        database
            .with_connection(|connection| {
                connection.execute_batch(
                    "BEGIN; CREATE TABLE IF NOT EXISTS __migration_probe (id INTEGER); ROLLBACK;",
                )?;
                Ok(())
            })
            .unwrap();
        let tables: i64 = database.with_connection(|c| c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('projects','workspaces','workspace_projects','repositories','repository_snapshots','trusted_repositories','identity_profiles','themes','global_settings')", [], |r| r.get(0))).unwrap();
        assert_eq!(tables, 9);
        let fk: i64 = database
            .with_connection(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM pragma_foreign_key_list('workspace_projects')",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(fk, 2);
    }

    #[test]
    fn migration_runner_is_safe_to_run_twice_and_creates_all_v2_tables() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        super::migrations::run(&mut connection).unwrap();
        super::migrations::run(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let names = [
            "projects",
            "workspaces",
            "workspace_projects",
            "repositories",
            "project_repositories",
            "repository_snapshots",
            "repository_remotes",
            "trusted_repositories",
            "identity_profiles",
            "repository_identity_bindings",
            "global_settings",
            "themes",
        ];
        for name in names {
            assert_eq!(
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                        [name],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                1,
                "missing {name}"
            );
        }
    }

    #[test]
    fn upgrading_v1_preserves_existing_rows() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(super::migrations::MIGRATION_1)
            .unwrap();
        connection.execute("INSERT INTO jobs (id,kind,title,status,created_at,updated_at) VALUES ('legacy','test','Legacy','queued','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')", []).unwrap();
        super::migrations::run(&mut connection).unwrap();
        let title: String = connection
            .query_row("SELECT title FROM jobs WHERE id='legacy'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "Legacy");
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
