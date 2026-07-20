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
        self.create_with_id(&uuid::Uuid::new_v4().to_string(), kind, title)
    }

    pub fn create_with_id(&self, id: &str, kind: &str, title: &str) -> Result<Job, AppError> {
        uuid::Uuid::parse_str(id)
            .map_err(|_| AppError::InvalidInput("job id must be a UUID".to_owned()))?;
        if kind.is_empty() || title.is_empty() {
            return Err(AppError::InvalidInput(
                "job kind and title are required".to_owned(),
            ));
        }
        let _guard = self.operation_lock.lock();
        if self.repository.get(id)?.is_some() {
            return Err(AppError::InvalidInput("job id already exists".to_owned()));
        }
        let now = chrono::Utc::now();
        let job = Job {
            id: id.to_owned(),
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

    pub fn fail_running_by_kind_prefix(
        &self,
        kind_prefix: &str,
        error: ErrorEnvelope,
    ) -> Result<Vec<Job>, AppError> {
        if kind_prefix.is_empty() {
            return Err(AppError::InvalidInput(
                "job kind prefix is required".to_owned(),
            ));
        }
        let _guard = self.operation_lock.lock();
        let running_ids = self
            .repository
            .list()?
            .into_iter()
            .filter(|job| {
                job.status == crate::jobs::model::JobStatus::Running
                    && job.kind.starts_with(kind_prefix)
            })
            .map(|job| job.id)
            .collect::<Vec<_>>();
        running_ids
            .into_iter()
            .map(|id| {
                self.transition(
                    &id,
                    crate::jobs::model::JobStatus::Failed,
                    Some(error.clone()),
                )
            })
            .collect()
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

    pub fn request_cancel(&self, id: &str) -> Result<Job, AppError> {
        let _guard = self.operation_lock.lock();
        let mut job = self.required(id)?;
        if !matches!(
            job.status,
            crate::jobs::model::JobStatus::Queued | crate::jobs::model::JobStatus::Running
        ) {
            return Ok(job);
        }
        if !job.cancel_requested {
            job.cancel_requested = true;
            job.updated_at = chrono::Utc::now();
            self.repository.update(&job)?;
        }
        Ok(job)
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
        job.cancel_requested =
            job.cancel_requested || target == crate::jobs::model::JobStatus::Canceled;
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

    #[test]
    fn cancel_request_is_idempotent_and_survives_start_until_terminal_cancellation() {
        let service = JobService::new(Database::open_in_memory().expect("database opens"));
        let job = service
            .create("git.transport.fetch", "Fetch origin")
            .expect("Job creates");
        let requested = service.request_cancel(&job.id).expect("cancel requests");
        assert_eq!(requested.status, JobStatus::Queued);
        assert!(requested.cancel_requested);
        assert!(service.request_cancel(&job.id).unwrap().cancel_requested);

        let running = service.start(&job.id).expect("Job starts");
        assert_eq!(running.status, JobStatus::Running);
        assert!(running.cancel_requested);
        let canceled = service.cancel(&job.id).expect("Job cancels");
        assert_eq!(canceled.status, JobStatus::Canceled);
        assert!(service.request_cancel(&job.id).unwrap().cancel_requested);
    }

    #[test]
    fn caller_owned_job_ids_are_uuid_validated_and_unique() {
        let service = JobService::new(Database::open_in_memory().expect("database opens"));
        let id = uuid::Uuid::new_v4().to_string();
        let job = service
            .create_with_id(&id, "git.transport.fetch", "Fetch origin")
            .unwrap();
        assert_eq!(job.id, id);
        assert!(matches!(
            service.create_with_id(&id, "git.transport.fetch", "Duplicate"),
            Err(AppError::InvalidInput(_))
        ));
        assert!(matches!(
            service.create_with_id("not-a-uuid", "git.transport.fetch", "Invalid"),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn running_transport_jobs_can_be_failed_by_kind_prefix() {
        let service = JobService::new(Database::open_in_memory().expect("database opens"));
        let transport = service.create("git.transport.fetch", "Fetch").unwrap();
        service.start(&transport.id).unwrap();
        let other = service.create("provider.sync", "Provider").unwrap();
        service.start(&other.id).unwrap();

        let failed = service
            .fail_running_by_kind_prefix(
                "git.transport.",
                crate::error::TransportFailure::interrupted().envelope(),
            )
            .unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].id, transport.id);
        assert_eq!(failed[0].status, JobStatus::Failed);
        assert_eq!(
            service
                .list()
                .unwrap()
                .into_iter()
                .find(|job| job.id == other.id)
                .unwrap()
                .status,
            JobStatus::Running
        );
    }
}
