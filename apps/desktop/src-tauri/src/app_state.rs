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
use crate::providers::adapter::ProviderAdapterRegistry;
use crate::providers::service::ProviderService;
use crate::providers::store::ProviderStore;
#[cfg(not(all(feature = "e2e", debug_assertions)))]
use crate::secrets::KeyringSecretStore;
#[cfg(all(feature = "e2e", debug_assertions))]
use crate::secrets::MemorySecretStore;
use crate::secrets::SecretStore;
use crate::themes::ThemeManager;

pub struct AppState {
    pub database: Database,
    pub git: GitService,
    pub identities: IdentityService,
    pub secrets: Arc<dyn SecretStore>,
    pub jobs: JobService,
    pub plugins: PluginRegistry,
    pub permissions: PermissionGateway,
    pub providers: ProviderService,
    pub themes: ThemeManager,
    #[cfg(all(feature = "e2e", debug_assertions))]
    pub(crate) e2e_app_data_root: PathBuf,
    #[cfg(all(feature = "e2e", debug_assertions))]
    pub(crate) e2e_database_path: PathBuf,
}

#[cfg(all(feature = "e2e", debug_assertions))]
pub(crate) const E2E_APP_DATA_PREFIX: &str = "git-ramus-wdio-profile-";
#[cfg(all(feature = "e2e", debug_assertions))]
const E2E_APP_DATA_ROOT_ENV: &str = "GIT_RAMUS_WDIO_PROFILE_ROOT";

impl AppState {
    pub fn bootstrap(app: &AppHandle) -> Result<Self, AppError> {
        #[cfg(all(feature = "e2e", debug_assertions))]
        let app_data = match resolve_e2e_app_data_override(
            std::env::var_os(E2E_APP_DATA_ROOT_ENV).as_deref(),
            &std::env::temp_dir(),
        )? {
            Some(path) => path,
            None => platform_app_data_dir(app)?,
        };
        #[cfg(not(all(feature = "e2e", debug_assertions)))]
        let app_data = platform_app_data_dir(app)?;
        std::fs::create_dir_all(&app_data)?;
        let plugin_root = bundled_plugin_root(app)?;
        let state = Self::from_paths(&app_data.join("git-ramus.db"), &plugin_root)?;
        state.identities.import_global_if_empty()?;
        Ok(state)
    }

    pub fn from_paths(database_path: &Path, plugin_root: &Path) -> Result<Self, AppError> {
        #[cfg(all(feature = "e2e", debug_assertions))]
        let e2e_app_data_root = database_path
            .parent()
            .ok_or_else(|| AppError::InvalidInput("database path has no parent".to_owned()))?
            .to_path_buf();
        #[cfg(all(feature = "e2e", debug_assertions))]
        let e2e_database_path = database_path.to_path_buf();
        let database = Database::open(database_path)?;
        let plugins = PluginRegistry::discover(plugin_root)?;
        let themes = ThemeManager::discover(database.clone(), &plugins)?;
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
        #[cfg(all(feature = "e2e", debug_assertions))]
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        #[cfg(not(all(feature = "e2e", debug_assertions)))]
        let secrets: Arc<dyn SecretStore> =
            Arc::new(KeyringSecretStore::new("io.git-ramus.desktop"));
        let provider_adapters = ProviderAdapterRegistry::from_plugins(database.clone(), &plugins)?;
        #[cfg(all(feature = "e2e", debug_assertions))]
        let provider_adapters = {
            let mut provider_adapters = provider_adapters;
            provider_adapters
                .replace_gitlab_for_e2e(Arc::new(crate::providers::e2e_adapter::E2eProvider));
            provider_adapters
        };
        let providers = ProviderService::new(
            ProviderStore::new(database.clone()),
            Arc::clone(&secrets),
            provider_adapters,
        );
        providers.retry_secret_cleanup()?;
        Ok(Self {
            jobs: JobService::new(database.clone()),
            git: GitService::with_write_locks(database.clone(), write_locks.clone()),
            identities: IdentityService::with_write_locks(database.clone(), write_locks),
            secrets,
            plugins,
            permissions,
            providers,
            themes,
            database,
            #[cfg(all(feature = "e2e", debug_assertions))]
            e2e_app_data_root,
            #[cfg(all(feature = "e2e", debug_assertions))]
            e2e_database_path,
        })
    }
}

#[cfg(test)]
const fn e2e_uses_memory_secret_store() -> bool {
    cfg!(all(feature = "e2e", debug_assertions))
}

fn platform_app_data_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    app.path()
        .app_data_dir()
        .map_err(|error| AppError::InvalidInput(error.to_string()))
}

#[cfg(all(feature = "e2e", debug_assertions))]
fn resolve_e2e_app_data_override(
    value: Option<&std::ffi::OsStr>,
    temp_root: &Path,
) -> Result<Option<PathBuf>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(AppError::InvalidInput(
            "E2E app-data override is empty".to_owned(),
        ));
    }
    let candidate = PathBuf::from(value);
    let metadata = std::fs::symlink_metadata(&candidate)?;
    if !metadata.is_dir() || is_symlink_or_reparse_point(&metadata) {
        return Err(AppError::InvalidInput(
            "E2E app-data override is not a safe directory".to_owned(),
        ));
    }
    let canonical_temp = std::fs::canonicalize(temp_root)?;
    let canonical_candidate = std::fs::canonicalize(candidate)?;
    let safe_parent = canonical_candidate.parent() == Some(canonical_temp.as_path());
    let safe_name = canonical_candidate
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(E2E_APP_DATA_PREFIX));
    if !safe_parent || !safe_name {
        return Err(AppError::InvalidInput(
            "E2E app-data override escaped the guarded temp boundary".to_owned(),
        ));
    }
    Ok(Some(canonical_candidate))
}

#[cfg(all(feature = "e2e", debug_assertions))]
fn is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
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

    #[cfg(all(feature = "e2e", debug_assertions))]
    use std::ffi::OsStr;

    use tempfile::tempdir;

    use super::AppState;

    #[cfg(all(feature = "e2e", debug_assertions))]
    use super::{E2E_APP_DATA_PREFIX, resolve_e2e_app_data_override};

    #[cfg(all(feature = "e2e", debug_assertions))]
    #[test]
    fn e2e_app_data_override_accepts_only_a_direct_prefixed_temp_directory() {
        let temp_root = std::env::temp_dir();
        let profile = tempfile::Builder::new()
            .prefix(E2E_APP_DATA_PREFIX)
            .tempdir_in(&temp_root)
            .expect("profile creates");
        let resolved =
            resolve_e2e_app_data_override(Some(profile.path().as_os_str()), temp_root.as_path())
                .expect("safe profile resolves")
                .expect("override is present");
        assert_eq!(
            resolved,
            fs::canonicalize(profile.path()).expect("profile canonicalizes")
        );

        let nested = profile.path().join("nested");
        fs::create_dir(&nested).expect("nested directory creates");
        assert!(
            resolve_e2e_app_data_override(Some(nested.as_os_str()), temp_root.as_path()).is_err()
        );

        let wrong_prefix = tempfile::Builder::new()
            .prefix("untrusted-app-data-")
            .tempdir_in(&temp_root)
            .expect("wrong-prefix directory creates");
        assert!(
            resolve_e2e_app_data_override(
                Some(wrong_prefix.path().as_os_str()),
                temp_root.as_path()
            )
            .is_err()
        );
        assert_eq!(
            resolve_e2e_app_data_override(None, temp_root.as_path()).expect("absence resolves"),
            None
        );
        assert!(resolve_e2e_app_data_override(Some(OsStr::new("")), temp_root.as_path()).is_err());
    }

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

    #[test]
    fn e2e_app_state_secret_store_matches_the_debug_feature_boundary() {
        assert_eq!(
            super::e2e_uses_memory_secret_store(),
            cfg!(all(feature = "e2e", debug_assertions))
        );
    }
}
