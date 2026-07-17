use crate::db::Database;
use crate::error::AppError;
use crate::plugins::manifest::PluginManifest;

#[derive(Clone)]
pub struct PermissionGateway {
    database: Database,
}

impl PermissionGateway {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn grant_manifest_permissions(&self, manifest: &PluginManifest) -> Result<(), AppError> {
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
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::plugins::manifest::PluginManifest;

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
}
