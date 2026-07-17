use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

use crate::db::Database;
use crate::error::{AppError, ErrorEnvelope};
use crate::jobs::model::Job;

#[derive(Clone)]
pub struct JobRepository {
    database: Database,
}

impl JobRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert(&self, job: &Job) -> Result<(), AppError> {
        let error_json = job.error.as_ref().map(serde_json::to_string).transpose()?;
        self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO jobs (id, kind, title, status, progress, cancel_requested, error_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    job.id,
                    job.kind,
                    job.title,
                    job.status.to_string(),
                    job.progress,
                    job.cancel_requested,
                    error_json,
                    job.created_at.to_rfc3339(),
                    job.updated_at.to_rfc3339()
                ],
            )?;
            Ok(())
        })
    }

    pub fn update(&self, job: &Job) -> Result<(), AppError> {
        let error_json = job.error.as_ref().map(serde_json::to_string).transpose()?;
        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE jobs SET status = ?2, progress = ?3, cancel_requested = ?4, error_json = ?5, updated_at = ?6 WHERE id = ?1",
                params![
                    job.id,
                    job.status.to_string(),
                    job.progress,
                    job.cancel_requested,
                    error_json,
                    job.updated_at.to_rfc3339()
                ],
            )?;
            Ok(())
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<Job>, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row("SELECT id, kind, title, status, progress, cancel_requested, error_json, created_at, updated_at FROM jobs WHERE id = ?1", [id], Self::map_job)
                .optional()
        })
    }

    pub fn list(&self) -> Result<Vec<Job>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare("SELECT id, kind, title, status, progress, cancel_requested, error_json, created_at, updated_at FROM jobs ORDER BY created_at DESC")?;
            statement.query_map([], Self::map_job)?.collect()
        })
    }

    fn map_job(row: &Row<'_>) -> Result<Job, rusqlite::Error> {
        let status: String = row.get(3)?;
        let error_json: Option<String> = row.get(6)?;
        let created_at: String = row.get(7)?;
        let updated_at: String = row.get(8)?;
        Ok(Job {
            id: row.get(0)?,
            kind: row.get(1)?,
            title: row.get(2)?,
            status: status.parse().map_err(|error: AppError| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(error))
            })?,
            progress: row.get(4)?,
            cancel_requested: row.get(5)?,
            error: error_json
                .map(|value| serde_json::from_str::<ErrorEnvelope>(&value))
                .transpose()
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        })
    }
}
