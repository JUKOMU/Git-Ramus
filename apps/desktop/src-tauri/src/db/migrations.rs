use rusqlite::Connection;

use crate::error::AppError;

pub(super) const MIGRATION_1: &str = include_str!("../../migrations/0001_core.sql");
const MIGRATION_2: &str = include_str!("../../migrations/0002_git_client.sql");

pub(super) fn run(connection: &mut Connection) -> Result<(), AppError> {
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current < 1 {
        connection.execute_batch(MIGRATION_1)?;
    }
    let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current < 2 {
        connection.execute_batch(MIGRATION_2)?;
    }
    Ok(())
}
