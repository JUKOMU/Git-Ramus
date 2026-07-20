use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

use super::model::{
    CloneOperation, CloneProjectTarget, CloneStage, ProfileDeletionImpact,
    RepositoryTransportBinding, TransportConfigRepair, TransportKind, TransportProfile,
    TransportProfileSummary,
};
use crate::db::{Database, map_constraint_error};
use crate::error::AppError;

#[derive(Clone)]
pub struct TransportStore {
    database: Database,
}

impl TransportStore {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn insert_profile(&self, profile: &TransportProfile) -> Result<(), AppError> {
        self.database
            .with_connection(|connection| {
                connection
                    .execute(
                        "INSERT INTO transport_profiles(id,display_name,kind,ssh_key_path,ssh_variant,ssh_identities_only,https_username,https_use_http_path,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                        params![
                            profile.id,
                            profile.display_name,
                            profile.kind.as_str(),
                            profile.ssh_key_path,
                            profile.ssh_variant,
                            profile.ssh_identities_only,
                            profile.https_username,
                            profile.https_use_http_path,
                            profile.created_at.to_rfc3339(),
                            profile.updated_at.to_rfc3339()
                        ],
                    )
                    .map(|_| ())
            })
            .map_err(|error| map_constraint_error(error, "transport profile"))
    }

    pub fn update_profile(&self, profile: &TransportProfile) -> Result<(), AppError> {
        let changed = self
            .database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE transport_profiles SET display_name=?2,kind=?3,ssh_key_path=?4,ssh_variant=?5,ssh_identities_only=?6,https_username=?7,https_use_http_path=?8,updated_at=?9 WHERE id=?1",
                    params![
                        profile.id,
                        profile.display_name,
                        profile.kind.as_str(),
                        profile.ssh_key_path,
                        profile.ssh_variant,
                        profile.ssh_identities_only,
                        profile.https_username,
                        profile.https_use_http_path,
                        profile.updated_at.to_rfc3339()
                    ],
                )
            })
            .map_err(|error| map_constraint_error(error, "transport profile"))?;
        ensure_changed(changed, "transport profile", &profile.id)
    }

    pub fn get_profile(&self, id: &str) -> Result<Option<TransportProfile>, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT id,display_name,kind,ssh_key_path,ssh_variant,ssh_identities_only,https_username,https_use_http_path,created_at,updated_at FROM transport_profiles WHERE id=?1",
                    [id],
                    map_profile,
                )
                .optional()
        })
    }

    pub fn list_profiles(&self) -> Result<Vec<TransportProfile>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id,display_name,kind,ssh_key_path,ssh_variant,ssh_identities_only,https_username,https_use_http_path,created_at,updated_at FROM transport_profiles ORDER BY display_name,id",
            )?;
            statement
                .query_map([], map_profile)
                .map(|rows| rows.collect())?
        })
    }

    pub fn list_profile_summaries(&self) -> Result<Vec<TransportProfileSummary>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT p.id,p.display_name,p.kind,p.ssh_key_path,p.ssh_variant,p.ssh_identities_only,p.https_username,p.https_use_http_path,p.created_at,p.updated_at,COUNT(b.repository_id) FROM transport_profiles p LEFT JOIN repository_transport_bindings b ON b.transport_profile_id=p.id GROUP BY p.id ORDER BY p.display_name,p.id",
            )?;
            statement
                .query_map([], |row| {
                    let profile = map_profile(row)?;
                    let bound_repository_count: i64 = row.get(10)?;
                    let available = match profile.kind {
                        TransportKind::Ssh => profile
                            .ssh_key_path
                            .as_deref()
                            .is_some_and(|path| Path::new(path).is_file()),
                        TransportKind::Https => true,
                    };
                    Ok(profile.summary(available, bound_repository_count))
                })
                .map(|rows| rows.collect())?
        })
    }

    pub fn delete_profile(&self, id: &str) -> Result<(), AppError> {
        let changed = self
            .database
            .with_connection(|connection| {
                connection.execute("DELETE FROM transport_profiles WHERE id=?1", [id])
            })
            .map_err(|error| map_constraint_error(error, "transport profile"))?;
        ensure_changed(changed, "transport profile", id)
    }

    pub fn get_binding(
        &self,
        repository_id: &str,
    ) -> Result<Option<RepositoryTransportBinding>, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT repository_id,transport_profile_id,before_config_json,applied_config_json,applied_config_hash,drift_status,bound_at,updated_at FROM repository_transport_bindings WHERE repository_id=?1",
                    [repository_id],
                    map_binding,
                )
                .optional()
        })
    }

    pub fn list_bindings_for_profile(
        &self,
        profile_id: &str,
    ) -> Result<Vec<RepositoryTransportBinding>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT repository_id,transport_profile_id,before_config_json,applied_config_json,applied_config_hash,drift_status,bound_at,updated_at FROM repository_transport_bindings WHERE transport_profile_id=?1 ORDER BY repository_id",
            )?;
            statement
                .query_map([profile_id], map_binding)
                .map(|rows| rows.collect())?
        })
    }

    pub fn upsert_binding(&self, binding: &RepositoryTransportBinding) -> Result<(), AppError> {
        let before_config = serde_json::to_string(&binding.before_config)?;
        let applied_config = serde_json::to_string(&binding.applied_config)?;
        self.database
            .with_connection(|connection| {
                connection
                    .execute(
                        "INSERT INTO repository_transport_bindings(repository_id,transport_profile_id,before_config_json,applied_config_json,applied_config_hash,drift_status,bound_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(repository_id) DO UPDATE SET transport_profile_id=excluded.transport_profile_id,before_config_json=excluded.before_config_json,applied_config_json=excluded.applied_config_json,applied_config_hash=excluded.applied_config_hash,drift_status=excluded.drift_status,bound_at=excluded.bound_at,updated_at=excluded.updated_at",
                        params![
                            binding.repository_id,
                            binding.transport_profile_id,
                            before_config,
                            applied_config,
                            binding.applied_config_hash,
                            binding.drift_status.as_str(),
                            binding.bound_at.to_rfc3339(),
                            binding.updated_at.to_rfc3339()
                        ],
                    )
                    .map(|_| ())
            })
            .map_err(|error| map_constraint_error(error, "repository transport binding"))
    }

    pub fn mark_binding_drifted(
        &self,
        repository_id: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE repository_transport_bindings SET drift_status='drifted',updated_at=?2 WHERE repository_id=?1",
                params![repository_id, updated_at.to_rfc3339()],
            )
        })?;
        ensure_changed(changed, "repository transport binding", repository_id)
    }

    pub fn delete_binding(&self, repository_id: &str) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "DELETE FROM repository_transport_bindings WHERE repository_id=?1",
                [repository_id],
            )
        })?;
        ensure_changed(changed, "repository transport binding", repository_id)
    }

    pub fn profile_deletion_impact(
        &self,
        profile_id: &str,
    ) -> Result<ProfileDeletionImpact, AppError> {
        if self.get_profile(profile_id)?.is_none() {
            return Err(AppError::NotFound(format!(
                "transport profile {profile_id}"
            )));
        }
        let repository_ids = self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT repository_id FROM repository_transport_bindings WHERE transport_profile_id=?1 ORDER BY repository_id",
            )?;
            statement
                .query_map([profile_id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()
        })?;
        Ok(ProfileDeletionImpact {
            profile_id: profile_id.to_owned(),
            repository_ids,
        })
    }

    pub fn insert_repair(&self, repair: &TransportConfigRepair) -> Result<(), AppError> {
        let before_config = serde_json::to_string(&repair.before_config)?;
        let attempted_config = serde_json::to_string(&repair.attempted_config)?;
        self.database
            .with_connection(|connection| {
                connection
                    .execute(
                        "INSERT INTO transport_config_repairs(id,repository_id,before_config_json,attempted_config_json,error_code,created_at,resolved_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        params![
                            repair.id,
                            repair.repository_id,
                            before_config,
                            attempted_config,
                            repair.error_code,
                            repair.created_at.to_rfc3339(),
                            repair.resolved_at.map(|value| value.to_rfc3339())
                        ],
                    )
                    .map(|_| ())
            })
            .map_err(|error| map_constraint_error(error, "transport config repair"))
    }

    pub fn repository_has_unresolved_repair(&self, repository_id: &str) -> Result<bool, AppError> {
        self.database.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM transport_config_repairs WHERE repository_id=?1 AND resolved_at IS NULL)",
                [repository_id],
                |row| row.get(0),
            )
        })
    }

    pub fn resolve_repair(
        &self,
        repair_id: &str,
        resolved_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE transport_config_repairs SET resolved_at=?2 WHERE id=?1 AND resolved_at IS NULL",
                params![repair_id, resolved_at.to_rfc3339()],
            )
        })?;
        ensure_changed(changed, "transport config repair", repair_id)
    }

    pub fn insert_clone_operation(&self, operation: &CloneOperation) -> Result<(), AppError> {
        let project_target = serde_json::to_string(&operation.project_target)?;
        self.database
            .with_connection(|connection| {
                connection
                    .execute(
                        "INSERT INTO git_clone_operations(operation_id,job_id,source_summary,intent_id,transport_profile_id,provider_instance_id,provider_account_id,provider_repository_id,staging_path,owner_marker_path,final_path,project_target_json,current_stage,filesystem_complete,repository_id,project_id,profile_applied,provider_binding_complete,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
                        params![
                            operation.operation_id,
                            operation.job_id,
                            operation.source_summary,
                            operation.intent_id,
                            operation.transport_profile_id,
                            operation.provider_instance_id,
                            operation.provider_account_id,
                            operation.provider_repository_id,
                            operation.staging_path,
                            operation.owner_marker_path,
                            operation.final_path,
                            project_target,
                            operation.current_stage.as_str(),
                            operation.filesystem_complete,
                            operation.repository_id,
                            operation.project_id,
                            operation.profile_applied,
                            operation.provider_binding_complete,
                            operation.created_at.to_rfc3339(),
                            operation.updated_at.to_rfc3339()
                        ],
                    )
                    .map(|_| ())
            })
            .map_err(|error| map_constraint_error(error, "Git clone operation"))
    }

    pub fn get_clone_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<CloneOperation>, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT operation_id,job_id,source_summary,intent_id,transport_profile_id,provider_instance_id,provider_account_id,provider_repository_id,staging_path,owner_marker_path,final_path,project_target_json,current_stage,filesystem_complete,repository_id,project_id,profile_applied,provider_binding_complete,created_at,updated_at FROM git_clone_operations WHERE operation_id=?1",
                    [operation_id],
                    map_clone_operation,
                )
                .optional()
        })
    }

    pub fn update_clone_stage(
        &self,
        operation_id: &str,
        stage: CloneStage,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE git_clone_operations SET current_stage=?2,updated_at=?3 WHERE operation_id=?1",
                params![operation_id, stage.as_str(), updated_at.to_rfc3339()],
            )
        })?;
        ensure_changed(changed, "Git clone operation", operation_id)
    }

    pub fn mark_clone_filesystem_complete(
        &self,
        operation_id: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        self.update_clone_flag(operation_id, "filesystem_complete", updated_at)
    }

    pub fn mark_clone_repository(
        &self,
        operation_id: &str,
        repository_id: &str,
        project_id: Option<&str>,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE git_clone_operations SET repository_id=?2,project_id=?3,updated_at=?4 WHERE operation_id=?1",
                params![operation_id, repository_id, project_id, updated_at.to_rfc3339()],
            )
        })?;
        ensure_changed(changed, "Git clone operation", operation_id)
    }

    pub fn mark_clone_project(
        &self,
        operation_id: &str,
        project_id: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE git_clone_operations SET project_id=?2,updated_at=?3 WHERE operation_id=?1",
                params![operation_id, project_id, updated_at.to_rfc3339()],
            )
        })?;
        ensure_changed(changed, "Git clone operation", operation_id)
    }

    pub fn mark_clone_profile(
        &self,
        operation_id: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        self.update_clone_flag(operation_id, "profile_applied", updated_at)
    }

    pub fn mark_clone_provider_binding(
        &self,
        operation_id: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        self.update_clone_flag(operation_id, "provider_binding_complete", updated_at)
    }

    fn update_clone_flag(
        &self,
        operation_id: &str,
        column: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let sql = match column {
            "filesystem_complete" => {
                "UPDATE git_clone_operations SET filesystem_complete=1,updated_at=?2 WHERE operation_id=?1"
            }
            "profile_applied" => {
                "UPDATE git_clone_operations SET profile_applied=1,updated_at=?2 WHERE operation_id=?1"
            }
            "provider_binding_complete" => {
                "UPDATE git_clone_operations SET provider_binding_complete=1,updated_at=?2 WHERE operation_id=?1"
            }
            _ => {
                return Err(AppError::InvalidInput(
                    "unknown Git clone operation flag".to_owned(),
                ));
            }
        };
        let changed = self.database.with_connection(|connection| {
            connection.execute(sql, params![operation_id, updated_at.to_rfc3339()])
        })?;
        ensure_changed(changed, "Git clone operation", operation_id)
    }

    pub fn list_incomplete_clone_operations(&self) -> Result<Vec<CloneOperation>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT operation_id,job_id,source_summary,intent_id,transport_profile_id,provider_instance_id,provider_account_id,provider_repository_id,staging_path,owner_marker_path,final_path,project_target_json,current_stage,filesystem_complete,repository_id,project_id,profile_applied,provider_binding_complete,created_at,updated_at FROM git_clone_operations WHERE current_stage NOT IN ('completed','failed','cancelled') ORDER BY created_at,operation_id",
            )?;
            statement
                .query_map([], map_clone_operation)
                .map(|rows| rows.collect())?
        })
    }

    pub fn delete_clone_operation(&self, operation_id: &str) -> Result<(), AppError> {
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "DELETE FROM git_clone_operations WHERE operation_id=?1",
                [operation_id],
            )
        })?;
        ensure_changed(changed, "Git clone operation", operation_id)
    }
}

fn ensure_changed(changed: usize, resource: &str, id: &str) -> Result<(), AppError> {
    if changed == 0 {
        return Err(AppError::NotFound(format!("{resource} {id}")));
    }
    Ok(())
}

fn date_time(value: String) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::parse_from_rfc3339(&value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn json_value<T: serde::de::DeserializeOwned>(value: String) -> Result<T, rusqlite::Error> {
    serde_json::from_str(&value)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn parse_value<T>(value: String) -> Result<T, rusqlite::Error>
where
    T: FromStr<Err = AppError>,
{
    T::from_str(&value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn map_profile(row: &Row<'_>) -> Result<TransportProfile, rusqlite::Error> {
    Ok(TransportProfile {
        id: row.get(0)?,
        display_name: row.get(1)?,
        kind: parse_value(row.get(2)?)?,
        ssh_key_path: row.get(3)?,
        ssh_variant: row.get(4)?,
        ssh_identities_only: row.get(5)?,
        https_username: row.get(6)?,
        https_use_http_path: row.get(7)?,
        created_at: date_time(row.get(8)?)?,
        updated_at: date_time(row.get(9)?)?,
    })
}

fn map_binding(row: &Row<'_>) -> Result<RepositoryTransportBinding, rusqlite::Error> {
    Ok(RepositoryTransportBinding {
        repository_id: row.get(0)?,
        transport_profile_id: row.get(1)?,
        before_config: json_value(row.get(2)?)?,
        applied_config: json_value(row.get(3)?)?,
        applied_config_hash: row.get(4)?,
        drift_status: parse_value(row.get(5)?)?,
        bound_at: date_time(row.get(6)?)?,
        updated_at: date_time(row.get(7)?)?,
    })
}

fn map_clone_operation(row: &Row<'_>) -> Result<CloneOperation, rusqlite::Error> {
    Ok(CloneOperation {
        operation_id: row.get(0)?,
        job_id: row.get(1)?,
        source_summary: row.get(2)?,
        intent_id: row.get(3)?,
        transport_profile_id: row.get(4)?,
        provider_instance_id: row.get(5)?,
        provider_account_id: row.get(6)?,
        provider_repository_id: row.get(7)?,
        staging_path: row.get(8)?,
        owner_marker_path: row.get(9)?,
        final_path: row.get(10)?,
        project_target: json_value::<CloneProjectTarget>(row.get(11)?)?,
        current_stage: parse_value(row.get(12)?)?,
        filesystem_complete: row.get(13)?,
        repository_id: row.get(14)?,
        project_id: row.get(15)?,
        profile_applied: row.get(16)?,
        provider_binding_complete: row.get(17)?,
        created_at: date_time(row.get(18)?)?,
        updated_at: date_time(row.get(19)?)?,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::TransportStore;
    use crate::db::Database;
    use crate::git::model::{Repository, RepositoryKind};
    use crate::git::repository::RepositoryRepository;
    use crate::git::transport::model::{
        CloneOperation, CloneProjectTarget, CloneStage, RepositoryTransportBinding,
        TransportConfigRepair, TransportConfigSnapshot, TransportDriftStatus, TransportProfile,
    };

    fn seed_repository(database: &Database, path: &str) -> Repository {
        let repository = Repository::new(path, "Transport repository", RepositoryKind::Normal);
        RepositoryRepository::new(database.clone())
            .create(&repository)
            .unwrap();
        repository
    }

    fn snapshot(key: &str, value: &str) -> TransportConfigSnapshot {
        let mut snapshot = TransportConfigSnapshot::empty();
        snapshot
            .values
            .insert(key.to_owned(), vec![value.to_owned()]);
        snapshot
    }

    fn binding(
        repository_id: &str,
        profile_id: &str,
        before_config: TransportConfigSnapshot,
        applied_config: TransportConfigSnapshot,
    ) -> RepositoryTransportBinding {
        let timestamp = Utc::now();
        RepositoryTransportBinding {
            repository_id: repository_id.to_owned(),
            transport_profile_id: profile_id.to_owned(),
            before_config,
            applied_config,
            applied_config_hash: "applied-hash".to_owned(),
            drift_status: TransportDriftStatus::Clean,
            bound_at: timestamp,
            updated_at: timestamp,
        }
    }

    fn seed_job(database: &Database, id: &str) {
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO jobs(id,kind,title,status,created_at,updated_at) VALUES(?1,'git.transport.clone','Clone','queued','2026-07-20T00:00:00Z','2026-07-20T00:00:00Z')",
                    [id],
                )
            })
            .unwrap();
    }

    fn clone_operation(operation_id: &str, job_id: &str) -> CloneOperation {
        let timestamp = Utc::now();
        CloneOperation {
            operation_id: operation_id.to_owned(),
            job_id: job_id.to_owned(),
            source_summary: "git.example.test/acme/repository".to_owned(),
            intent_id: None,
            transport_profile_id: None,
            provider_instance_id: None,
            provider_account_id: None,
            provider_repository_id: None,
            staging_path: "/tmp/.git-ramus-clone-operation".to_owned(),
            owner_marker_path: "/tmp/.git-ramus-clone-operation.owner".to_owned(),
            final_path: "/tmp/repository".to_owned(),
            project_target: CloneProjectTarget::New {
                name: "Repository".to_owned(),
            },
            current_stage: CloneStage::Validating,
            filesystem_complete: false,
            repository_id: None,
            project_id: None,
            profile_applied: false,
            provider_binding_complete: false,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    #[test]
    fn profile_binding_and_clone_recovery_round_trip_without_exposing_key_paths() {
        let database = Database::open_in_memory().unwrap();
        let repository = seed_repository(&database, "/tmp/transport-store");
        let store = TransportStore::new(database.clone());
        let profile = TransportProfile::new_ssh("Work", "/keys/id_ed25519", true);
        store.insert_profile(&profile).unwrap();
        let binding = binding(
            &repository.id,
            &profile.id,
            TransportConfigSnapshot::empty(),
            snapshot("core.sshCommand", "ssh -i /keys/id_ed25519"),
        );
        store.upsert_binding(&binding).unwrap();
        assert_eq!(store.get_binding(&repository.id).unwrap(), Some(binding));
        assert_eq!(
            store
                .profile_deletion_impact(&profile.id)
                .unwrap()
                .repository_ids,
            vec![repository.id.clone()]
        );

        let operation_id = uuid::Uuid::new_v4().to_string();
        let job_id = uuid::Uuid::new_v4().to_string();
        seed_job(&database, &job_id);
        let clone = clone_operation(&operation_id, &job_id);
        store.insert_clone_operation(&clone).unwrap();
        assert_eq!(
            store
                .get_clone_operation(&operation_id)
                .unwrap()
                .unwrap()
                .final_path,
            clone.final_path
        );

        let serialized = serde_json::to_string(&store.list_profile_summaries().unwrap()).unwrap();
        assert!(!serialized.contains("/keys/id_ed25519"));
        assert!(serialized.contains("id_ed25519"));
    }

    #[test]
    fn bound_profiles_and_unresolved_repairs_block_destructive_changes() {
        let database = Database::open_in_memory().unwrap();
        let repository = seed_repository(&database, "/tmp/transport-repair");
        let store = TransportStore::new(database);
        let profile = TransportProfile::new_https("Work", "worker");
        store.insert_profile(&profile).unwrap();
        store
            .upsert_binding(&binding(
                &repository.id,
                &profile.id,
                TransportConfigSnapshot::empty(),
                snapshot("credential.useHttpPath", "true"),
            ))
            .unwrap();
        assert!(store.delete_profile(&profile.id).is_err());

        let timestamp = Utc::now();
        let repair = TransportConfigRepair {
            id: uuid::Uuid::new_v4().to_string(),
            repository_id: repository.id.clone(),
            before_config: TransportConfigSnapshot::empty(),
            attempted_config: snapshot("credential.useHttpPath", "true"),
            error_code: "git.transport.partial".to_owned(),
            created_at: timestamp,
            resolved_at: None,
        };
        store.insert_repair(&repair).unwrap();
        assert!(
            store
                .repository_has_unresolved_repair(&repository.id)
                .unwrap()
        );
        store.resolve_repair(&repair.id, Utc::now()).unwrap();
        assert!(
            !store
                .repository_has_unresolved_repair(&repository.id)
                .unwrap()
        );
    }

    #[test]
    fn profile_binding_and_clone_update_methods_preserve_invariants() {
        let database = Database::open_in_memory().unwrap();
        let repository = seed_repository(&database, "/tmp/transport-updates");
        let store = TransportStore::new(database.clone());
        let mut profile = TransportProfile::new_https("Original", "worker");
        store.insert_profile(&profile).unwrap();
        profile.display_name = "Updated".to_owned();
        profile.updated_at = Utc::now();
        store.update_profile(&profile).unwrap();
        assert_eq!(
            store
                .get_profile(&profile.id)
                .unwrap()
                .unwrap()
                .display_name,
            "Updated"
        );
        let value = binding(
            &repository.id,
            &profile.id,
            TransportConfigSnapshot::empty(),
            snapshot("credential.useHttpPath", "true"),
        );
        store.upsert_binding(&value).unwrap();
        assert_eq!(
            store.list_bindings_for_profile(&profile.id).unwrap(),
            vec![value]
        );
        store
            .mark_binding_drifted(&repository.id, Utc::now())
            .unwrap();
        assert_eq!(
            store
                .get_binding(&repository.id)
                .unwrap()
                .unwrap()
                .drift_status,
            TransportDriftStatus::Drifted
        );

        let operation_id = uuid::Uuid::new_v4().to_string();
        let job_id = uuid::Uuid::new_v4().to_string();
        seed_job(&database, &job_id);
        store
            .insert_clone_operation(&clone_operation(&operation_id, &job_id))
            .unwrap();
        store
            .update_clone_stage(&operation_id, CloneStage::Registering, Utc::now())
            .unwrap();
        store
            .mark_clone_filesystem_complete(&operation_id, Utc::now())
            .unwrap();
        store
            .mark_clone_repository(&operation_id, &repository.id, None, Utc::now())
            .unwrap();
        store.mark_clone_profile(&operation_id, Utc::now()).unwrap();
        store
            .mark_clone_provider_binding(&operation_id, Utc::now())
            .unwrap();
        let updated = store.get_clone_operation(&operation_id).unwrap().unwrap();
        assert!(updated.filesystem_complete);
        assert!(updated.profile_applied);
        assert!(updated.provider_binding_complete);
        assert_eq!(
            updated.repository_id.as_deref(),
            Some(repository.id.as_str())
        );
        assert_eq!(store.list_incomplete_clone_operations().unwrap().len(), 1);
        store
            .update_clone_stage(&operation_id, CloneStage::Completed, Utc::now())
            .unwrap();
        assert!(store.list_incomplete_clone_operations().unwrap().is_empty());
        store.delete_clone_operation(&operation_id).unwrap();

        store.delete_binding(&repository.id).unwrap();
        store.delete_profile(&profile.id).unwrap();
        assert!(store.get_profile(&profile.id).unwrap().is_none());
    }
}
