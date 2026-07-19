use crate::db::Database;
use crate::error::AppError;
use crate::plugins::manifest::{PluginKind, PluginManifest};

#[derive(Clone)]
pub struct PermissionGateway {
    database: Database,
}

impl PermissionGateway {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn grant_manifest_permissions(&self, manifest: &PluginManifest) -> Result<(), AppError> {
        if manifest.kind != PluginKind::Builtin {
            return Err(AppError::PermissionDenied);
        }
        let granted_at = chrono::Utc::now().to_rfc3339();
        self.database.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            for permission in &manifest.permissions {
                for resource in &permission.resources {
                    transaction.execute(
                        "INSERT INTO permission_grants (plugin_id, capability, resource, granted_at, revoked_at) VALUES (?1, ?2, ?3, ?4, NULL) ON CONFLICT(plugin_id, capability, resource) DO UPDATE SET granted_at = excluded.granted_at, revoked_at = NULL",
                        rusqlite::params![manifest.id, permission.capability, resource, granted_at],
                    )?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn is_allowed(
        &self,
        plugin_id: &str,
        capability: &str,
        resource: &str,
    ) -> Result<bool, AppError> {
        self.database.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM permission_grants WHERE plugin_id = ?1 AND capability = ?2 AND resource = ?3 AND revoked_at IS NULL)",
                rusqlite::params![plugin_id, capability, resource],
                |row| row.get(0),
            )
        })
    }

    pub fn revoke(
        &self,
        plugin_id: &str,
        capability: &str,
        resource: &str,
    ) -> Result<(), AppError> {
        let revoked_at = chrono::Utc::now().to_rfc3339();
        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE permission_grants SET revoked_at = ?4 WHERE plugin_id = ?1 AND capability = ?2 AND resource = ?3",
                rusqlite::params![plugin_id, capability, resource, revoked_at],
            )?;
            Ok(())
        })
    }

    pub fn grant_dynamic(
        &self,
        plugin_id: &str,
        capability: &str,
        resource: &str,
    ) -> Result<(), AppError> {
        validate_exact_resource(resource)?;
        let granted_at = chrono::Utc::now().to_rfc3339();
        let changed = self.database.with_connection(|connection| {
            connection.execute(
                "INSERT INTO permission_grants(plugin_id,capability,resource,granted_at,revoked_at)
                 SELECT plugin_id,?2,?3,?4,NULL FROM plugin_installations
                 WHERE plugin_id=?1 AND enabled=1
                 ON CONFLICT(plugin_id,capability,resource)
                 DO UPDATE SET granted_at=excluded.granted_at,revoked_at=NULL",
                rusqlite::params![plugin_id, capability, resource, granted_at],
            )
        })?;
        if changed == 0 {
            return Err(AppError::NotFound("enabled plugin installation".to_owned()));
        }
        Ok(())
    }

    pub fn list_active_resources(
        &self,
        plugin_id: &str,
        capability: &str,
        prefix: &str,
    ) -> Result<Vec<String>, AppError> {
        if prefix.is_empty() || prefix.chars().any(char::is_control) {
            return Err(AppError::InvalidInput(
                "permission resource prefix is invalid".to_owned(),
            ));
        }
        self.database.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT resource FROM permission_grants
                 WHERE plugin_id=?1 AND capability=?2 AND revoked_at IS NULL
                 ORDER BY resource",
            )?;
            let resources = statement
                .query_map(rusqlite::params![plugin_id, capability], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(resources
                .into_iter()
                .filter(|resource| resource.starts_with(prefix))
                .collect())
        })
    }

    pub fn revoke_dynamic(
        &self,
        plugin_id: &str,
        capability: &str,
        resource: &str,
    ) -> Result<(), AppError> {
        validate_exact_resource(resource)?;
        self.revoke(plugin_id, capability, resource)
    }

    pub fn revoke_resource_for_all(
        &self,
        capability: &str,
        resource: &str,
    ) -> Result<(), AppError> {
        validate_exact_resource(resource)?;
        let revoked_at = chrono::Utc::now().to_rfc3339();
        self.database.with_connection(|connection| {
            connection.execute(
                "UPDATE permission_grants SET revoked_at=?3
                 WHERE capability=?1 AND resource=?2 AND revoked_at IS NULL",
                rusqlite::params![capability, resource, revoked_at],
            )?;
            Ok(())
        })
    }
}

fn validate_exact_resource(resource: &str) -> Result<(), AppError> {
    if resource.is_empty()
        || resource.chars().any(char::is_control)
        || resource.contains('*')
        || resource.split('/').any(|component| component == "..")
    {
        return Err(AppError::InvalidInput(
            "permission resource is invalid".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::db::Database;
    use crate::plugins::manifest::PluginManifest;
    use crate::plugins::registry::PluginRegistry;
    use tempfile::tempdir;

    use super::PermissionGateway;

    fn manifest() -> PluginManifest {
        serde_json::from_str(include_str!(
            "../../../../../plugins/builtin-welcome/plugin.json"
        ))
        .expect("manifest parses")
    }

    fn seed_installation(database: &Database) {
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO plugin_installations (plugin_id, version, kind, root_path, installed_at, updated_at) VALUES ('git-ramus.welcome', '0.1.0', 'builtin', '/builtin/welcome', '2026-07-17T00:00:00Z', '2026-07-17T00:00:00Z')",
                    [],
                )?;
                Ok(())
            })
            .expect("installation seeds");
    }

    #[test]
    fn grants_only_manifest_requested_resources() {
        let database = Database::open_in_memory().expect("database opens");
        seed_installation(&database);
        let gateway = PermissionGateway::new(database);
        gateway
            .grant_manifest_permissions(&manifest())
            .expect("grants seed");
        assert!(
            gateway
                .is_allowed("git-ramus.welcome", "app:read", "info")
                .expect("check")
        );
        assert!(
            !gateway
                .is_allowed("git-ramus.welcome", "app:read", "secrets")
                .expect("check")
        );
    }

    #[test]
    fn revocation_takes_effect_immediately() {
        let database = Database::open_in_memory().expect("database opens");
        seed_installation(&database);
        let gateway = PermissionGateway::new(database);
        gateway
            .grant_manifest_permissions(&manifest())
            .expect("grants seed");
        gateway
            .revoke("git-ramus.welcome", "tasks:create", "echo")
            .expect("revoke succeeds");
        assert!(
            !gateway
                .is_allowed("git-ramus.welcome", "tasks:create", "echo")
                .expect("check")
        );
    }

    #[test]
    fn undeclared_capability_is_never_granted() {
        let database = Database::open_in_memory().expect("database opens");
        seed_installation(&database);
        let gateway = PermissionGateway::new(database);
        gateway
            .grant_manifest_permissions(&manifest())
            .expect("grants seed");
        assert!(
            !gateway
                .is_allowed("git-ramus.welcome", "fs:write", "*")
                .expect("check")
        );
    }

    #[test]
    fn an_external_provider_request_is_not_a_grant_until_an_account_is_selected() {
        let directory = tempdir().expect("temporary directory creates");
        let plugin = directory.path().join("example.reader");
        fs::create_dir(&plugin).unwrap();
        fs::write(
            plugin.join("plugin.json"),
            r#"{"schemaVersion":1,"id":"example.reader","name":"Reader","version":"0.1.0","publisher":"example","description":"Provider reader","kind":"external","sdkVersion":"^0.1.0","entrypoints":{"ui":"ui.html"},"contributions":{"navigation":[]},"permissions":[{"capability":"providers:read","resources":["providers"]}]}"#,
        )
        .unwrap();
        fs::write(plugin.join("ui.html"), "<p>reader</p>").unwrap();
        let registry = PluginRegistry::discover(directory.path()).unwrap();
        let database = Database::open_in_memory().unwrap();
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('example.reader','0.1.0','external','/external/reader',1,'2026-07-19T00:00:00Z','2026-07-19T00:00:00Z')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let gateway = PermissionGateway::new(database);
        let resource = "provider-account/7f3c0214-373c-4d43-b0c7-cdaed1cbcc50";

        assert!(registry.manifest_requests("example.reader", "providers:read", "providers"));
        assert!(
            !gateway
                .is_allowed("example.reader", "providers:read", resource)
                .unwrap()
        );
        gateway
            .grant_dynamic("example.reader", "providers:read", resource)
            .unwrap();
        assert!(
            gateway
                .is_allowed("example.reader", "providers:read", resource)
                .unwrap()
        );
        assert_eq!(
            gateway
                .list_active_resources("example.reader", "providers:read", "provider-account/")
                .unwrap(),
            [resource]
        );
        gateway
            .revoke_dynamic("example.reader", "providers:read", resource)
            .unwrap();
        assert!(
            !gateway
                .is_allowed("example.reader", "providers:read", resource)
                .unwrap()
        );
    }
}
