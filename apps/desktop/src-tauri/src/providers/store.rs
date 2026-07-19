use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

use crate::db::{Database, map_constraint_error};
use crate::error::AppError;
use crate::git::model::Remote;
use crate::providers::model::{
    AccountDeletionImpact, AccountDeletionResolution, NewProviderAccount, ProviderAccount,
    ProviderBinding, ProviderInstance, SecretCleanupRecord,
};

#[derive(Clone)]
pub struct ProviderStore {
    database: Database,
}

impl ProviderStore {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn insert_instance(&self, value: ProviderInstance) -> Result<ProviderInstance, AppError> {
        self.database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO provider_instances(id,provider_kind,display_name,base_url,api_base_url,custom_ca_path,last_validated_at,server_version,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![
                        value.id,
                        value.provider_kind.as_str(),
                        value.display_name,
                        value.base_url,
                        value.api_base_url,
                        value.custom_ca_path,
                        value.last_validated_at.map(|time| time.to_rfc3339()),
                        value.server_version,
                        value.created_at.to_rfc3339(),
                        value.updated_at.to_rfc3339()
                    ],
                )?;
                Ok(())
            })
            .map_err(|error| map_constraint_error(error, "Provider instance"))?;
        self.get_instance(&value.id)
    }

    pub fn update_instance(&self, value: ProviderInstance) -> Result<ProviderInstance, AppError> {
        let changed = self
            .database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE provider_instances SET provider_kind=?2,display_name=?3,base_url=?4,api_base_url=?5,custom_ca_path=?6,last_validated_at=?7,server_version=?8,updated_at=?9 WHERE id=?1",
                    params![
                        value.id,
                        value.provider_kind.as_str(),
                        value.display_name,
                        value.base_url,
                        value.api_base_url,
                        value.custom_ca_path,
                        value.last_validated_at.map(|time| time.to_rfc3339()),
                        value.server_version,
                        value.updated_at.to_rfc3339()
                    ],
                )
            })
            .map_err(|error| map_constraint_error(error, "Provider instance"))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!(
                "Provider instance {}",
                value.id
            )));
        }
        self.get_instance(&value.id)
    }

    pub fn get_instance(&self, id: &str) -> Result<ProviderInstance, AppError> {
        self.database
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT id,provider_kind,display_name,base_url,api_base_url,custom_ca_path,last_validated_at,server_version,created_at,updated_at FROM provider_instances WHERE id=?1",
                        [id],
                        map_instance,
                    )
                    .optional()
            })?
            .ok_or_else(|| AppError::NotFound(format!("Provider instance {id}")))
    }

    pub fn list_instances(&self) -> Result<Vec<ProviderInstance>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id,provider_kind,display_name,base_url,api_base_url,custom_ca_path,last_validated_at,server_version,created_at,updated_at FROM provider_instances ORDER BY display_name,id",
            )?;
            statement
                .query_map([], map_instance)
                .map(|rows| rows.collect())?
        })
    }

    pub fn delete_instance(&self, id: &str) -> Result<(), AppError> {
        let changed = self
            .database
            .with_connection(|connection| {
                connection.execute("DELETE FROM provider_instances WHERE id=?1", [id])
            })
            .map_err(|error| map_constraint_error(error, "Provider instance"))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("Provider instance {id}")));
        }
        Ok(())
    }

    pub fn insert_account(&self, value: NewProviderAccount) -> Result<ProviderAccount, AppError> {
        let account = self
            .database
            .with_immediate_transaction(|transaction| {
                let account_count: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM provider_accounts WHERE instance_id=?1",
                    [&value.instance_id],
                    |row| row.get(0),
                )?;
                transaction.execute(
                    "INSERT INTO provider_accounts(id,instance_id,provider_user_id,username,display_name,avatar_url,secret_ref,is_default,last_validated_at,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![
                        value.id,
                        value.instance_id,
                        value.provider_user_id,
                        value.username,
                        value.display_name,
                        value.avatar_url,
                        value.secret_ref,
                        account_count == 0,
                        value.last_validated_at.to_rfc3339(),
                        value.created_at.to_rfc3339(),
                        value.updated_at.to_rfc3339()
                    ],
                )?;
                transaction
                    .query_row(
                        "SELECT id,instance_id,provider_user_id,username,display_name,avatar_url,secret_ref,is_default,last_validated_at,created_at,updated_at FROM provider_accounts WHERE id=?1",
                        [&value.id],
                        map_account,
                    )
                    .map_err(AppError::from)
            })
            .map_err(|error| map_constraint_error(error, "Provider account"))?;
        Ok(account)
    }

    pub fn update_account_secret(
        &self,
        id: &str,
        secret_ref: &str,
        validated_at: &str,
    ) -> Result<(), AppError> {
        let changed = self
            .database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE provider_accounts SET secret_ref=?2,last_validated_at=?3,updated_at=?3 WHERE id=?1",
                    params![id, secret_ref, validated_at],
                )
            })
            .map_err(|error| map_constraint_error(error, "Provider account"))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("Provider account {id}")));
        }
        Ok(())
    }

    pub fn set_default_account(&self, instance_id: &str, account_id: &str) -> Result<(), AppError> {
        self.database.with_immediate_transaction(|transaction| {
            let belongs: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE id=?1 AND instance_id=?2)",
                params![account_id, instance_id],
                |row| row.get(0),
            )?;
            if !belongs {
                return Err(AppError::InvalidInput(
                    "default Provider account must belong to its instance".to_owned(),
                ));
            }
            transaction.execute(
                "UPDATE provider_accounts SET is_default=0 WHERE instance_id=?1",
                [instance_id],
            )?;
            transaction.execute(
                "UPDATE provider_accounts SET is_default=1 WHERE id=?1 AND instance_id=?2",
                params![account_id, instance_id],
            )?;
            Ok(())
        })
    }

    pub fn get_account(&self, id: &str) -> Result<ProviderAccount, AppError> {
        self.database
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT id,instance_id,provider_user_id,username,display_name,avatar_url,secret_ref,is_default,last_validated_at,created_at,updated_at FROM provider_accounts WHERE id=?1",
                        [id],
                        map_account,
                    )
                    .optional()
            })?
            .ok_or_else(|| AppError::NotFound(format!("Provider account {id}")))
    }

    pub fn list_accounts(&self, instance_id: &str) -> Result<Vec<ProviderAccount>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id,instance_id,provider_user_id,username,display_name,avatar_url,secret_ref,is_default,last_validated_at,created_at,updated_at FROM provider_accounts WHERE instance_id=?1 ORDER BY is_default DESC,username,id",
            )?;
            statement
                .query_map([instance_id], map_account)
                .map(|rows| rows.collect())?
        })
    }

    pub fn account_deletion_impact(&self, id: &str) -> Result<AccountDeletionImpact, AppError> {
        let account = self.get_account(id)?;
        let explicit_binding_count = self.database.with_connection(|connection| {
            connection.query_row(
                "SELECT COUNT(*) FROM provider_repository_bindings WHERE provider_account_id=?1",
                [id],
                |row| row.get(0),
            )
        })?;
        let inherited_binding_count = if account.is_default {
            self.database.with_connection(|connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM provider_repository_bindings WHERE provider_instance_id=?1 AND provider_account_id IS NULL",
                    [&account.instance_id],
                    |row| row.get(0),
                )
            })?
        } else {
            0
        };
        let sibling_account_ids: Vec<String> = self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id FROM provider_accounts WHERE instance_id=?1 AND id<>?2 ORDER BY id",
            )?;
            statement
                .query_map(params![account.instance_id, id], |row| row.get(0))
                .map(|rows| rows.collect())?
        })?;
        Ok(AccountDeletionImpact {
            account_id: account.id,
            instance_id: account.instance_id,
            is_default: account.is_default,
            explicit_binding_count,
            inherited_binding_count,
            requires_new_default: account.is_default && !sibling_account_ids.is_empty(),
            sibling_account_ids,
        })
    }

    pub fn delete_account_with_resolution(
        &self,
        id: &str,
        resolution: &AccountDeletionResolution,
        new_default_account_id: Option<&str>,
    ) -> Result<(), AppError> {
        self.database.with_immediate_transaction(|transaction| {
            let account = transaction
                .query_row(
                    "SELECT id,instance_id,provider_user_id,username,display_name,avatar_url,secret_ref,is_default,last_validated_at,created_at,updated_at FROM provider_accounts WHERE id=?1",
                    [id],
                    map_account,
                )
                .optional()?
                .ok_or_else(|| AppError::NotFound(format!("Provider account {id}")))?;
            let sibling_account_ids = {
                let mut statement = transaction.prepare(
                    "SELECT id FROM provider_accounts WHERE instance_id=?1 AND id<>?2 ORDER BY id",
                )?;
                statement
                    .query_map(params![account.instance_id, id], |row| row.get(0))?
                    .collect::<Result<Vec<String>, _>>()?
            };

            match resolution {
                AccountDeletionResolution::Reassign { account_id } => {
                    if !sibling_account_ids.contains(account_id) {
                        return Err(AppError::InvalidInput(
                            "replacement Provider account must belong to the same instance"
                                .to_owned(),
                        ));
                    }
                    transaction.execute(
                        "UPDATE provider_repository_bindings SET provider_account_id=?2,updated_at=?3 WHERE provider_account_id=?1",
                        params![id, account_id, Utc::now().to_rfc3339()],
                    )?;
                }
                AccountDeletionResolution::Inherit => {
                    transaction.execute(
                        "UPDATE provider_repository_bindings SET provider_account_id=NULL,updated_at=?2 WHERE provider_account_id=?1",
                        params![id, Utc::now().to_rfc3339()],
                    )?;
                }
                AccountDeletionResolution::Unbind => {
                    transaction.execute(
                        "DELETE FROM provider_repository_bindings WHERE provider_account_id=?1",
                        [id],
                    )?;
                    if account.is_default && sibling_account_ids.is_empty() {
                        transaction.execute(
                            "DELETE FROM provider_repository_bindings WHERE provider_instance_id=?1 AND provider_account_id IS NULL",
                            [&account.instance_id],
                        )?;
                    }
                }
            }

            if account.is_default && !sibling_account_ids.is_empty() {
                let Some(new_default) = new_default_account_id else {
                    return Err(AppError::InvalidInput(
                        "deleting a default Provider account requires a replacement".to_owned(),
                    ));
                };
                if !sibling_account_ids.iter().any(|candidate| candidate == new_default) {
                    return Err(AppError::InvalidInput(
                        "replacement default Provider account must belong to the same instance"
                            .to_owned(),
                    ));
                }
                transaction.execute(
                    "UPDATE provider_accounts SET is_default=0 WHERE instance_id=?1",
                    [&account.instance_id],
                )?;
                transaction.execute(
                    "UPDATE provider_accounts SET is_default=1 WHERE id=?1 AND instance_id=?2",
                    params![new_default, account.instance_id],
                )?;
            } else if new_default_account_id.is_some() {
                return Err(AppError::InvalidInput(
                    "replacement default Provider account is not applicable".to_owned(),
                ));
            }

            transaction.execute("DELETE FROM provider_accounts WHERE id=?1", [id])?;
            let resource = format!("provider-account/{id}");
            transaction.execute(
                "UPDATE permission_grants SET revoked_at=?2 WHERE resource=?1 AND revoked_at IS NULL",
                params![resource, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
    }

    pub fn upsert_binding(&self, value: ProviderBinding) -> Result<ProviderBinding, AppError> {
        self.database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO provider_repository_bindings(repository_id,remote_name,provider_instance_id,provider_account_id,provider_repository_id,full_name,web_url,matched_url,binding_source,bound_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(repository_id,remote_name) DO UPDATE SET provider_instance_id=excluded.provider_instance_id,provider_account_id=excluded.provider_account_id,provider_repository_id=excluded.provider_repository_id,full_name=excluded.full_name,web_url=excluded.web_url,matched_url=excluded.matched_url,binding_source=excluded.binding_source,bound_at=excluded.bound_at,updated_at=excluded.updated_at",
                    params![
                        value.repository_id,
                        value.remote_name,
                        value.provider_instance_id,
                        value.provider_account_id,
                        value.provider_repository_id,
                        value.full_name,
                        value.web_url,
                        value.matched_url,
                        value.binding_source.as_str(),
                        value.bound_at.to_rfc3339(),
                        value.updated_at.to_rfc3339()
                    ],
                )?;
                Ok(())
            })
            .map_err(|error| map_constraint_error(error, "Provider repository binding"))?;
        self.get_binding(&value.repository_id, &value.remote_name)?
            .ok_or_else(|| AppError::NotFound("Provider repository binding".to_owned()))
    }

    pub fn get_binding(
        &self,
        repository_id: &str,
        remote_name: &str,
    ) -> Result<Option<ProviderBinding>, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT repository_id,remote_name,provider_instance_id,provider_account_id,provider_repository_id,full_name,web_url,matched_url,binding_source,bound_at,updated_at FROM provider_repository_bindings WHERE repository_id=?1 AND remote_name=?2",
                    params![repository_id, remote_name],
                    map_binding,
                )
                .optional()
        })
    }

    pub fn list_bindings(
        &self,
        instance_id: Option<&str>,
    ) -> Result<Vec<ProviderBinding>, AppError> {
        self.database.with_connection(|connection| {
            if let Some(instance_id) = instance_id {
                let mut statement = connection.prepare(
                    "SELECT repository_id,remote_name,provider_instance_id,provider_account_id,provider_repository_id,full_name,web_url,matched_url,binding_source,bound_at,updated_at FROM provider_repository_bindings WHERE provider_instance_id=?1 ORDER BY repository_id,remote_name",
                )?;
                statement
                    .query_map([instance_id], map_binding)
                    .map(|rows| rows.collect())?
            } else {
                let mut statement = connection.prepare(
                    "SELECT repository_id,remote_name,provider_instance_id,provider_account_id,provider_repository_id,full_name,web_url,matched_url,binding_source,bound_at,updated_at FROM provider_repository_bindings ORDER BY repository_id,remote_name",
                )?;
                statement
                    .query_map([], map_binding)
                    .map(|rows| rows.collect())?
            }
        })
    }

    pub fn list_bindings_for_account(
        &self,
        account_id: &str,
    ) -> Result<Vec<ProviderBinding>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT b.repository_id,b.remote_name,b.provider_instance_id,b.provider_account_id,b.provider_repository_id,b.full_name,b.web_url,b.matched_url,b.binding_source,b.bound_at,b.updated_at
                 FROM provider_repository_bindings b
                 JOIN provider_accounts a ON a.id=?1
                 WHERE b.provider_account_id=?1
                    OR (a.is_default=1 AND b.provider_account_id IS NULL AND b.provider_instance_id=a.instance_id)
                 ORDER BY b.repository_id,b.remote_name",
            )?;
            statement
                .query_map([account_id], map_binding)
                .map(|rows| rows.collect())?
        })
    }

    pub fn list_local_remotes(&self) -> Result<Vec<Remote>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT repository_id,name,fetch_url,push_url FROM repository_remotes ORDER BY repository_id,name",
            )?;
            statement
                .query_map([], |row| {
                    Ok(Remote {
                        repository_id: row.get(0)?,
                        name: row.get(1)?,
                        fetch_url: row.get(2)?,
                        push_url: row.get(3)?,
                    })
                })
                .map(|rows| rows.collect())?
        })
    }

    pub fn delete_binding(&self, repository_id: &str, remote_name: &str) -> Result<(), AppError> {
        self.database.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM provider_repository_bindings WHERE repository_id=?1 AND remote_name=?2",
                    params![repository_id, remote_name],
                )
                .map(|_| ())
        })
    }

    pub fn enqueue_secret_cleanup(&self, secret_ref: &str) -> Result<(), AppError> {
        self.database.with_connection(|connection| {
            connection
                .execute(
                    "INSERT INTO provider_secret_cleanup(secret_ref,created_at) VALUES(?1,?2) ON CONFLICT(secret_ref) DO NOTHING",
                    params![secret_ref, Utc::now().to_rfc3339()],
                )
                .map(|_| ())
        })
    }

    pub fn list_secret_cleanup(&self) -> Result<Vec<SecretCleanupRecord>, AppError> {
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT secret_ref,created_at,last_attempt_at,attempt_count,last_error_code FROM provider_secret_cleanup ORDER BY created_at,secret_ref",
            )?;
            statement
                .query_map([], map_cleanup_record)
                .map(|rows| rows.collect())?
        })
    }

    pub fn record_cleanup_attempt(
        &self,
        secret_ref: &str,
        succeeded: bool,
        error_code: Option<&str>,
    ) -> Result<(), AppError> {
        if succeeded && error_code.is_some() {
            return Err(AppError::InvalidInput(
                "successful cleanup cannot include an error code".to_owned(),
            ));
        }
        if !succeeded && !error_code.is_some_and(is_stable_error_code) {
            return Err(AppError::InvalidInput(
                "cleanup failure requires a stable error code".to_owned(),
            ));
        }
        self.database.with_immediate_transaction(|transaction| {
            if succeeded {
                let referenced: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE secret_ref=?1)",
                    [secret_ref],
                    |row| row.get(0),
                )?;
                if referenced {
                    return Err(AppError::InvalidInput(
                        "referenced Provider secret cannot be cleaned up".to_owned(),
                    ));
                }
                let changed = transaction.execute(
                    "DELETE FROM provider_secret_cleanup WHERE secret_ref=?1",
                    [secret_ref],
                )?;
                if changed == 0 {
                    return Err(AppError::NotFound("Provider secret cleanup".to_owned()));
                }
            } else {
                let changed = transaction.execute(
                    "UPDATE provider_secret_cleanup SET last_attempt_at=?2,attempt_count=attempt_count+1,last_error_code=?3 WHERE secret_ref=?1",
                    params![secret_ref, Utc::now().to_rfc3339(), error_code],
                )?;
                if changed == 0 {
                    return Err(AppError::NotFound("Provider secret cleanup".to_owned()));
                }
            }
            Ok(())
        })
    }

    pub fn secret_ref_is_referenced(&self, secret_ref: &str) -> Result<bool, AppError> {
        self.database.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE secret_ref=?1)",
                [secret_ref],
                |row| row.get(0),
            )
        })
    }
}

fn parse_datetime(value: String) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::parse_from_rfc3339(&value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn map_instance(row: &Row<'_>) -> Result<ProviderInstance, rusqlite::Error> {
    let kind: String = row.get(1)?;
    Ok(ProviderInstance {
        id: row.get(0)?,
        provider_kind: kind
            .parse()
            .map_err(|error: AppError| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        display_name: row.get(2)?,
        base_url: row.get(3)?,
        api_base_url: row.get(4)?,
        custom_ca_path: row.get(5)?,
        last_validated_at: row
            .get::<_, Option<String>>(6)?
            .map(parse_datetime)
            .transpose()?,
        server_version: row.get(7)?,
        created_at: parse_datetime(row.get(8)?)?,
        updated_at: parse_datetime(row.get(9)?)?,
    })
}

fn map_account(row: &Row<'_>) -> Result<ProviderAccount, rusqlite::Error> {
    Ok(ProviderAccount {
        id: row.get(0)?,
        instance_id: row.get(1)?,
        provider_user_id: row.get(2)?,
        username: row.get(3)?,
        display_name: row.get(4)?,
        avatar_url: row.get(5)?,
        secret_ref: row.get(6)?,
        is_default: row.get(7)?,
        last_validated_at: parse_datetime(row.get(8)?)?,
        created_at: parse_datetime(row.get(9)?)?,
        updated_at: parse_datetime(row.get(10)?)?,
    })
}

fn map_binding(row: &Row<'_>) -> Result<ProviderBinding, rusqlite::Error> {
    let source: String = row.get(8)?;
    Ok(ProviderBinding {
        repository_id: row.get(0)?,
        remote_name: row.get(1)?,
        provider_instance_id: row.get(2)?,
        provider_account_id: row.get(3)?,
        provider_repository_id: row.get(4)?,
        full_name: row.get(5)?,
        web_url: row.get(6)?,
        matched_url: row.get(7)?,
        binding_source: source
            .parse()
            .map_err(|error: AppError| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
        bound_at: parse_datetime(row.get(9)?)?,
        updated_at: parse_datetime(row.get(10)?)?,
    })
}

fn map_cleanup_record(row: &Row<'_>) -> Result<SecretCleanupRecord, rusqlite::Error> {
    Ok(SecretCleanupRecord {
        secret_ref: row.get(0)?,
        created_at: parse_datetime(row.get(1)?)?,
        last_attempt_at: row
            .get::<_, Option<String>>(2)?
            .map(parse_datetime)
            .transpose()?,
        attempt_count: row.get(3)?,
        last_error_code: row.get(4)?,
    })
}

fn is_stable_error_code(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some(character) if character.is_ascii_lowercase())
        && value.len() <= 128
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '.'
                || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use crate::db::Database;
    use crate::providers::model::{
        AccountDeletionResolution, BindingSource, NewProviderAccount, ProviderBinding,
        ProviderInstance, ProviderKind,
    };

    use super::ProviderStore;

    const REPOSITORY_ID: &str = "cf7f409b-f5b2-443a-8cff-099cc0bc8032";

    fn instance(kind: ProviderKind, base_url: &str) -> ProviderInstance {
        let now = Utc::now();
        ProviderInstance {
            id: Uuid::new_v4().to_string(),
            provider_kind: kind,
            display_name: "Provider".to_owned(),
            base_url: base_url.to_owned(),
            api_base_url: format!("{base_url}/api/v4"),
            custom_ca_path: None,
            last_validated_at: None,
            server_version: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn new_account(instance_id: &str, provider_user_id: &str) -> NewProviderAccount {
        let now = Utc::now();
        NewProviderAccount {
            id: Uuid::new_v4().to_string(),
            instance_id: instance_id.to_owned(),
            provider_user_id: provider_user_id.to_owned(),
            username: format!("user-{provider_user_id}"),
            display_name: None,
            avatar_url: None,
            secret_ref: format!("provider/account/{}/{}", Uuid::new_v4(), Uuid::new_v4()),
            last_validated_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    fn seed_local_remote(database: &Database, remote_name: &str) {
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT OR IGNORE INTO repositories(id,canonical_path,display_name,kind,created_at,updated_at) VALUES(?1,'C:/repository','Repository','normal','2026-07-19T00:00:00Z','2026-07-19T00:00:00Z')",
                    [REPOSITORY_ID],
                )?;
                connection.execute(
                    "INSERT INTO repository_remotes(repository_id,name,fetch_url,push_url) VALUES(?1,?2,'https://gitlab.example/group/repository.git','git@gitlab.example:group/repository.git')",
                    rusqlite::params![REPOSITORY_ID, remote_name],
                )?;
                Ok(())
            })
            .expect("remote seeds");
    }

    fn binding(remote_name: &str, instance_id: &str, account_id: Option<&str>) -> ProviderBinding {
        let now = Utc::now();
        ProviderBinding {
            repository_id: REPOSITORY_ID.to_owned(),
            remote_name: remote_name.to_owned(),
            provider_instance_id: instance_id.to_owned(),
            provider_account_id: account_id.map(str::to_owned),
            provider_repository_id: "42".to_owned(),
            full_name: "group/repository".to_owned(),
            web_url: "https://gitlab.example/group/repository".to_owned(),
            matched_url: "gitlab.example/group/repository".to_owned(),
            binding_source: BindingSource::Auto,
            bound_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn one_default_account_and_same_instance_bindings_are_enforced() {
        let store = ProviderStore::new(Database::open_in_memory().expect("database opens"));
        let github = store
            .insert_instance(instance(ProviderKind::Github, "https://github.com"))
            .unwrap();
        let gitlab = store
            .insert_instance(instance(ProviderKind::Gitlab, "https://gitlab.com"))
            .unwrap();
        let first = store.insert_account(new_account(&github.id, "1")).unwrap();
        let second = store.insert_account(new_account(&github.id, "2")).unwrap();
        assert!(first.is_default);
        assert!(!second.is_default);
        store.set_default_account(&github.id, &second.id).unwrap();
        assert!(store.get_account(&second.id).unwrap().is_default);

        let foreign = store.insert_account(new_account(&gitlab.id, "9")).unwrap();
        seed_local_remote(store.database(), "origin");
        assert!(
            store
                .upsert_binding(binding("origin", &github.id, Some(&foreign.id)))
                .is_err()
        );
        assert_eq!(store.list_accounts(&github.id).unwrap().len(), 2);

        let raw_second_default = store.database().with_connection(|connection| {
            connection.execute(
                "UPDATE provider_accounts SET is_default=1 WHERE id=?1",
                [&first.id],
            )
        });
        assert!(raw_second_default.is_err());
    }

    #[test]
    fn deleting_a_local_remote_cascades_only_its_provider_binding() {
        let store = ProviderStore::new(Database::open_in_memory().expect("database opens"));
        let instance = store
            .insert_instance(instance(ProviderKind::Gitlab, "https://gitlab.example"))
            .unwrap();
        let account = store
            .insert_account(new_account(&instance.id, "1"))
            .unwrap();
        seed_local_remote(store.database(), "origin");
        seed_local_remote(store.database(), "upstream");
        store
            .upsert_binding(binding("origin", &instance.id, Some(&account.id)))
            .unwrap();
        store
            .upsert_binding(binding("upstream", &instance.id, Some(&account.id)))
            .unwrap();

        store
            .database()
            .with_connection(|connection| {
                connection.execute(
                    "DELETE FROM repository_remotes WHERE repository_id=?1 AND name='origin'",
                    [REPOSITORY_ID],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(
            store
                .get_binding(REPOSITORY_ID, "origin")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_binding(REPOSITORY_ID, "upstream")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn account_binding_views_move_only_inherited_bindings_when_the_default_changes() {
        let store = ProviderStore::new(Database::open_in_memory().expect("database opens"));
        let instance = store
            .insert_instance(instance(ProviderKind::Gitlab, "https://gitlab.example"))
            .unwrap();
        let first = store
            .insert_account(new_account(&instance.id, "1"))
            .unwrap();
        let second = store
            .insert_account(new_account(&instance.id, "2"))
            .unwrap();
        for name in ["first", "second", "inherited"] {
            seed_local_remote(store.database(), name);
        }
        store
            .upsert_binding(binding("first", &instance.id, Some(&first.id)))
            .unwrap();
        store
            .upsert_binding(binding("second", &instance.id, Some(&second.id)))
            .unwrap();
        store
            .upsert_binding(binding("inherited", &instance.id, None))
            .unwrap();

        assert_eq!(
            store
                .list_bindings_for_account(&first.id)
                .unwrap()
                .into_iter()
                .map(|binding| binding.remote_name)
                .collect::<Vec<_>>(),
            ["first", "inherited"]
        );
        assert_eq!(
            store
                .list_bindings_for_account(&second.id)
                .unwrap()
                .into_iter()
                .map(|binding| binding.remote_name)
                .collect::<Vec<_>>(),
            ["second"]
        );

        store.set_default_account(&instance.id, &second.id).unwrap();
        assert_eq!(
            store
                .list_bindings_for_account(&first.id)
                .unwrap()
                .into_iter()
                .map(|binding| binding.remote_name)
                .collect::<Vec<_>>(),
            ["first"]
        );
        assert_eq!(
            store
                .list_bindings_for_account(&second.id)
                .unwrap()
                .into_iter()
                .map(|binding| binding.remote_name)
                .collect::<Vec<_>>(),
            ["inherited", "second"]
        );
    }

    #[test]
    fn deletion_impact_resolution_and_dynamic_grant_revocation_are_atomic() {
        let store = ProviderStore::new(Database::open_in_memory().expect("database opens"));
        let instance = store
            .insert_instance(instance(ProviderKind::Gitlab, "https://gitlab.example"))
            .unwrap();
        let first = store
            .insert_account(new_account(&instance.id, "1"))
            .unwrap();
        let second = store
            .insert_account(new_account(&instance.id, "2"))
            .unwrap();
        seed_local_remote(store.database(), "origin");
        seed_local_remote(store.database(), "upstream");
        store
            .upsert_binding(binding("origin", &instance.id, Some(&first.id)))
            .unwrap();
        store
            .upsert_binding(binding("upstream", &instance.id, None))
            .unwrap();
        store
            .database()
            .with_connection(|connection| {
                connection.execute("INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('example.reader','0.1.0','external','C:/plugin',1,'2026-07-19T00:00:00Z','2026-07-19T00:00:00Z')", [])?;
                connection.execute("INSERT INTO permission_grants(plugin_id,capability,resource,granted_at) VALUES('example.reader','providers:read',?1,'2026-07-19T00:00:00Z')", [format!("provider-account/{}", first.id)])?;
                Ok(())
            })
            .unwrap();

        let impact = store.account_deletion_impact(&first.id).unwrap();
        assert_eq!(impact.explicit_binding_count, 1);
        assert_eq!(impact.inherited_binding_count, 1);
        assert!(impact.requires_new_default);

        store
            .delete_account_with_resolution(
                &first.id,
                &AccountDeletionResolution::Inherit,
                Some(&second.id),
            )
            .unwrap();

        assert!(store.get_account(&second.id).unwrap().is_default);
        assert!(
            store
                .get_binding(REPOSITORY_ID, "origin")
                .unwrap()
                .unwrap()
                .provider_account_id
                .is_none()
        );
        let revoked: Option<String> = store
            .database()
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT revoked_at FROM permission_grants WHERE resource=?1",
                    [format!("provider-account/{}", first.id)],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert!(revoked.is_some());
    }

    #[test]
    fn cleanup_cannot_remove_a_still_referenced_secret_ref() {
        let store = ProviderStore::new(Database::open_in_memory().expect("database opens"));
        let instance = store
            .insert_instance(instance(ProviderKind::Gitlab, "https://gitlab.example"))
            .unwrap();
        let account = store
            .insert_account(new_account(&instance.id, "1"))
            .unwrap();
        store.enqueue_secret_cleanup(&account.secret_ref).unwrap();
        assert!(
            store
                .record_cleanup_attempt(&account.secret_ref, true, None)
                .is_err()
        );
        assert_eq!(store.list_secret_cleanup().unwrap().len(), 1);
        store
            .record_cleanup_attempt(&account.secret_ref, false, Some("secret-store.unavailable"))
            .unwrap();
        let record = store.list_secret_cleanup().unwrap().remove(0);
        assert_eq!(record.attempt_count, 1);
        assert_eq!(
            record.last_error_code.as_deref(),
            Some("secret-store.unavailable")
        );
    }
}
