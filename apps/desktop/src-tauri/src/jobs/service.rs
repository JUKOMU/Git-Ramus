use std::sync::Arc;

use parking_lot::Mutex;

use crate::db::Database;
use crate::error::{AppError, ErrorEnvelope};
use crate::jobs::model::Job;
use crate::jobs::repository::JobRepository;

#[derive(Clone)]
pub struct JobService {
    repository: JobRepository,
    operation_lock: Arc<Mutex<()>>,
}

impl JobService {
    pub fn new(database: Database) -> Self {
        Self {
            repository: JobRepository::new(database),
            operation_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn create(&self, kind: &str, title: &str) -> Result<Job, AppError> {
        let _guard = self.operation_lock.lock();
        let now = chrono::Utc::now();
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.to_owned(),
            title: title.to_owned(),
            status: crate::jobs::model::JobStatus::Queued,
            progress: 0.0,
            cancel_requested: false,
            created_at: now,
            updated_at: now,
            error: None,
        };
        self.repository.insert(&job)?;
        Ok(job)
    }

    pub fn start(&self, id: &str) -> Result<Job, AppError> {
        let _guard = self.operation_lock.lock();
        self.transition(id, crate::jobs::model::JobStatus::Running, None)
    }

    pub fn succeed(&self, id: &str) -> Result<Job, AppError> {
        let _guard = self.operation_lock.lock();
        let mut job = self.transition(id, crate::jobs::model::JobStatus::Succeeded, None)?;
        job.progress = 1.0;
        job.updated_at = chrono::Utc::now();
        self.repository.update(&job)?;
        Ok(job)
    }

    pub fn fail(&self, id: &str, error: ErrorEnvelope) -> Result<Job, AppError> {
        let _guard = self.operation_lock.lock();
        self.transition(id, crate::jobs::model::JobStatus::Failed, Some(error))
    }

    pub fn cancel(&self, id: &str) -> Result<Job, AppError> {
        let _guard = self.operation_lock.lock();
        self.transition(id, crate::jobs::model::JobStatus::Canceled, None)
    }

    pub fn set_progress(&self, id: &str, progress: f64) -> Result<Job, AppError> {
        let _guard = self.operation_lock.lock();
        if !(0.0..=1.0).contains(&progress) {
            return Err(AppError::InvalidInput(
                "job progress must be between 0 and 1".to_owned(),
            ));
        }
        let mut job = self.required(id)?;
        if job.status != crate::jobs::model::JobStatus::Running {
            return Err(AppError::InvalidInput(
                "only running jobs accept progress".to_owned(),
            ));
        }
        job.progress = progress;
        job.updated_at = chrono::Utc::now();
        self.repository.update(&job)?;
        Ok(job)
    }

    pub fn list(&self) -> Result<Vec<Job>, AppError> {
        let _guard = self.operation_lock.lock();
        self.repository.list()
    }

    pub fn is_canceled(&self, id: &str) -> Result<bool, AppError> {
        let _guard = self.operation_lock.lock();
        Ok(self.required(id)?.status == crate::jobs::model::JobStatus::Canceled)
    }

    fn transition(
        &self,
        id: &str,
        target: crate::jobs::model::JobStatus,
        error: Option<ErrorEnvelope>,
    ) -> Result<Job, AppError> {
        let mut job = self.required(id)?;
        let valid = matches!(
            (job.status, target),
            (
                crate::jobs::model::JobStatus::Queued,
                crate::jobs::model::JobStatus::Running
            ) | (
                crate::jobs::model::JobStatus::Queued,
                crate::jobs::model::JobStatus::Canceled
            ) | (
                crate::jobs::model::JobStatus::Running,
                crate::jobs::model::JobStatus::Succeeded
            ) | (
                crate::jobs::model::JobStatus::Running,
                crate::jobs::model::JobStatus::Failed
            ) | (
                crate::jobs::model::JobStatus::Running,
                crate::jobs::model::JobStatus::Canceled
            )
        );
        if !valid {
            return Err(AppError::InvalidInput(format!(
                "job transition {} -> {} is invalid",
                job.status, target
            )));
        }
        job.status = target;
        job.cancel_requested = target == crate::jobs::model::JobStatus::Canceled;
        job.error = error;
        job.updated_at = chrono::Utc::now();
        self.repository.update(&job)?;
        Ok(job)
    }

    fn required(&self, id: &str) -> Result<Job, AppError> {
        self.repository
            .get(id)?
            .ok_or_else(|| AppError::NotFound(format!("job {id}")))
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::error::AppError;
    use crate::jobs::model::JobStatus;

    use super::JobService;

    #[test]
    fn job_moves_through_valid_lifecycle() {
        let service = JobService::new(Database::open_in_memory().expect("database opens"));
        let created = service
            .create("system.echo", "Echo hello")
            .expect("job creates");
        assert_eq!(created.status, JobStatus::Queued);
        let running = service.start(&created.id).expect("job starts");
        assert_eq!(running.status, JobStatus::Running);
        assert_eq!(
            service
                .set_progress(&created.id, 0.5)
                .expect("progress")
                .progress,
            0.5
        );
        let complete = service.succeed(&created.id).expect("job completes");
        assert_eq!(complete.status, JobStatus::Succeeded);
        assert_eq!(complete.progress, 1.0);
    }

    #[test]
    fn terminal_job_rejects_another_transition() {
        let service = JobService::new(Database::open_in_memory().expect("database opens"));
        let job = service
            .create("system.echo", "Echo hello")
            .expect("job creates");
        service.cancel(&job.id).expect("job cancels");
        let error = service
            .start(&job.id)
            .expect_err("terminal transition fails");
        assert!(matches!(error, AppError::InvalidInput(_)));
    }

    #[test]
    fn canceled_running_job_cannot_be_resurrected_by_progress() {
        let service = JobService::new(Database::open_in_memory().expect("database opens"));
        let job = service
            .create("system.echo", "Echo hello")
            .expect("job creates");
        service.start(&job.id).expect("job starts");
        service.cancel(&job.id).expect("job cancels");
        let error = service
            .set_progress(&job.id, 0.75)
            .expect_err("canceled job rejects progress");
        assert!(matches!(error, AppError::InvalidInput(_)));
        assert!(service.is_canceled(&job.id).expect("canceled state reads"));
    }
}
