use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::db::Database;
use crate::error::AppError;
use crate::git::repository::RepositoryWriteLocks;
use crate::git::service::GitService;
use crate::identity::IdentityService;
use crate::jobs::JobService;
use crate::plugins::PluginRegistry;
use crate::plugins::manifest::PluginKind;
use crate::plugins::permissions::PermissionGateway;
use crate::secrets::{KeyringSecretStore, SecretStore};

pub struct AppState {
    pub database: Database,
    pub git: GitService,
    pub identities: IdentityService,
    pub secrets: Arc<dyn SecretStore>,
    pub jobs: JobService,
    pub plugins: PluginRegistry,
    pub permissions: PermissionGateway,
}

impl AppState {
    pub fn bootstrap(app: &AppHandle) -> Result<Self, AppError> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|error| AppError::InvalidInput(error.to_string()))?;
        std::fs::create_dir_all(&app_data)?;
        let plugin_root = bundled_plugin_root(app)?;
        let state = Self::from_paths(&app_data.join("git-ramus.db"), &plugin_root)?;
        state.identities.import_global_if_empty()?;
        Ok(state)
    }

    pub fn from_paths(database_path: &Path, plugin_root: &Path) -> Result<Self, AppError> {
        let database = Database::open(database_path)?;
        let plugins = PluginRegistry::discover(plugin_root)?;
        let permissions = PermissionGateway::new(database.clone());
        let now = chrono::Utc::now().to_rfc3339();
        for descriptor in plugins.descriptors() {
            let manifest = &descriptor.manifest;
            let root_path = plugin_root
                .join(&manifest.id)
                .to_string_lossy()
                .into_owned();
            let kind = match manifest.kind {
                PluginKind::Builtin => "builtin",
                PluginKind::External => "external",
            };
            let is_new = database.with_connection(|connection| {
                let inserted = connection.execute(
                    "INSERT INTO plugin_installations (plugin_id, version, kind, root_path, installed_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5) ON CONFLICT(plugin_id) DO NOTHING",
                    rusqlite::params![
                        &manifest.id,
                        &manifest.version,
                        kind,
                        &root_path,
                        &now
                    ],
                )?;
                connection.execute(
                    "UPDATE plugin_installations SET version = ?2, kind = ?3, root_path = ?4, updated_at = ?5 WHERE plugin_id = ?1",
                    rusqlite::params![
                        &manifest.id,
                        &manifest.version,
                        kind,
                        &root_path,
                        &now
                    ],
                )?;
                Ok(inserted == 1)
            })?;
            if is_new && manifest.kind == PluginKind::Builtin {
                permissions.grant_manifest_permissions(manifest)?;
            }
        }
        let write_locks = RepositoryWriteLocks::default();
        Ok(Self {
            jobs: JobService::new(database.clone()),
            git: GitService::with_write_locks(database.clone(), write_locks.clone()),
            identities: IdentityService::with_write_locks(database.clone(), write_locks),
            secrets: Arc::new(KeyringSecretStore::new("io.git-ramus.desktop")),
            plugins,
            permissions,
            database,
        })
    }
}

fn bundled_plugin_root(app: &AppHandle) -> Result<PathBuf, AppError> {
    if cfg!(debug_assertions) {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/plugins"))
    } else {
        app.path()
            .resource_dir()
            .map(|path| path.join("resources/plugins"))
            .map_err(|error| AppError::InvalidInput(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::AppState;

    fn write_builtin_plugin(root: &Path) {
        let plugin = root.join("git-ramus.welcome");
        fs::create_dir_all(&plugin).expect("plugin directory creates");
        fs::write(
            plugin.join("plugin.json"),
            r#"{"schemaVersion":1,"id":"git-ramus.welcome","name":"Welcome","version":"0.1.0","publisher":"git-ramus","description":"Welcome plugin","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{"ui":"ui.html"},"contributions":{"navigation":[]},"permissions":[{"capability":"app:read","resources":["info"]}]}"#,
        )
        .expect("manifest writes");
        fs::write(plugin.join("ui.html"), "<h1>Welcome</h1>").expect("UI writes");
    }

    #[test]
    fn revoked_builtin_permission_stays_revoked_after_bootstrap() {
        let directory = tempdir().expect("temp directory creates");
        let plugin_root = directory.path().join("plugins");
        write_builtin_plugin(&plugin_root);
        let database_path = directory.path().join("git-ramus.db");

        let first = AppState::from_paths(&database_path, &plugin_root).expect("bootstrap succeeds");
        assert!(
            first
                .permissions
                .is_allowed("git-ramus.welcome", "app:read", "info")
                .expect("permission reads")
        );
        first
            .permissions
            .revoke("git-ramus.welcome", "app:read", "info")
            .expect("permission revokes");
        drop(first);

        let second =
            AppState::from_paths(&database_path, &plugin_root).expect("second bootstrap succeeds");
        assert!(
            !second
                .permissions
                .is_allowed("git-ramus.welcome", "app:read", "info")
                .expect("permission reads")
        );
    }
}
