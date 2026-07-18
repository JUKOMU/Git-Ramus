mod migrations;

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::error::AppError;

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
}
