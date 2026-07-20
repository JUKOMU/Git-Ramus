use rusqlite::Connection;

use crate::error::AppError;

pub(super) const MIGRATION_1: &str = include_str!("../../migrations/0001_core.sql");
pub(super) const MIGRATION_2: &str = include_str!("../../migrations/0002_git_client.sql");
pub(super) const MIGRATION_3: &str = include_str!("../../migrations/0003_provider_discovery.sql");
pub(super) const MIGRATION_4: &str = include_str!("../../migrations/0004_git_transport.sql");

pub(super) fn run(connection: &mut Connection) -> Result<(), AppError> {
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current < 1 {
        connection.execute_batch(MIGRATION_1)?;
    }
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current < 2 {
        connection.execute_batch(MIGRATION_2)?;
    }
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current < 3 {
        connection.execute_batch(MIGRATION_3)?;
    }
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current < 4 {
        connection.execute_batch(MIGRATION_4)?;
    }
    Ok(())
}
