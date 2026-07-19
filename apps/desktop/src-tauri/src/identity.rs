#[cfg(test)]
mod roundtrip_tests {
    use crate::db::Database;

    #[test]
    fn identity_and_theme_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let identities = super::IdentityProfileRepository::new(db.clone());
        let themes = super::ThemeRepository::new(db);
        let mut profile = super::IdentityProfile::new("Alice", "alice", "alice@example.com");
        profile.gpg_format = Some("ssh".into());
        profile.signing_key = Some("key".into());
        profile.sign_commits = true;
        profile.sign_tags = true;
        identities.create(&profile).unwrap();
        let loaded_profile = identities.get(&profile.id).unwrap();
        assert_eq!(loaded_profile.display_name, "Alice");
        assert_eq!(loaded_profile.gpg_format.as_deref(), Some("ssh"));
        assert!(loaded_profile.sign_commits && loaded_profile.sign_tags);
        assert_eq!(loaded_profile.created_at, profile.created_at);
        let mut theme = super::Theme::new("dark", "builtin", "1.0", "{}");
        theme.is_valid = false;
        themes.create(&theme).unwrap();
        let loaded_theme = themes.get(&theme.theme_id).unwrap();
        assert_eq!(loaded_theme.theme_id, "dark");
        assert_eq!(loaded_theme.definition_json, "{}");
        assert!(!loaded_theme.is_valid);
        assert_eq!(loaded_theme.updated_at, theme.updated_at);
    }
}
use crate::{
    db::{Database, map_constraint_error},
    error::AppError,
    git::engine::{GitCommand, GitOutput, GitRunner, SystemGitRunner},
    git::model::IdentityBinding,
    git::repository::{
        IdentityBindingRepository, RepositoryRepository, RepositoryWriteLocks, TrustRepository,
    },
};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityProfile {
    pub id: String,
    pub display_name: String,
    pub user_name: String,
    pub user_email: String,
    pub gpg_format: Option<String>,
    pub signing_key: Option<String>,
    pub sign_commits: bool,
    pub sign_tags: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityProfileInput {
    pub display_name: String,
    pub user_name: String,
    pub user_email: String,
    pub gpg_format: Option<String>,
    pub signing_key: Option<String>,
    #[serde(default)]
    pub sign_commits: bool,
    #[serde(default)]
    pub sign_tags: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IdentitySource {
    GlobalProfile,
    RepositoryProfile,
    SelectedProfile,
    ExternalGlobal,
    ExternalLocal,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityDriftField {
    pub key: String,
    pub expected: Vec<String>,
    pub actual: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityDrift {
    pub fields: Vec<IdentityDriftField>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveIdentity {
    pub repository_id: String,
    pub profile_id: Option<String>,
    pub profile: Option<IdentityProfile>,
    pub source: IdentitySource,
    pub display_name: String,
    pub user_name: String,
    pub user_email: String,
    pub gpg_format: Option<String>,
    pub signing_key: Option<String>,
    pub sign_commits: bool,
    pub sign_tags: bool,
    pub drift: Option<IdentityDrift>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitIdentity {
    pub profile_id: Option<String>,
    pub user_name: String,
    pub user_email: String,
    pub gpg_format: Option<String>,
    pub signing_key: Option<String>,
    pub sign_commits: bool,
}

impl CommitIdentity {
    pub fn command_scope_args(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        append_config_override(&mut args, "user.name", &self.user_name);
        append_config_override(&mut args, "user.email", &self.user_email);
        if let Some(format) = &self.gpg_format {
            append_config_override(&mut args, "gpg.format", format);
        }
        if let Some(signing_key) = &self.signing_key {
            append_config_override(&mut args, "user.signingKey", signing_key);
        }
        append_config_override(
            &mut args,
            "commit.gpgSign",
            if self.sign_commits { "true" } else { "false" },
        );
        args
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningToolRequest {
    pub format: String,
    pub program: String,
    pub signing_key: String,
    pub repository_path: PathBuf,
}

pub trait SigningToolLocator: Send + Sync {
    fn ensure_available(&self, request: &SigningToolRequest) -> Result<(), AppError>;
}

#[derive(Debug, Default)]
pub struct PathSigningToolLocator;

impl SigningToolLocator for PathSigningToolLocator {
    fn ensure_available(&self, request: &SigningToolRequest) -> Result<(), AppError> {
        if !program_exists(&request.program, &request.repository_path) {
            return Err(AppError::UserActionRequired(format!(
                "signing tool is unavailable for {}",
                request.format
            )));
        }
        if request.format == "ssh" && !request.signing_key.starts_with("key::") {
            let configured = Path::new(&request.signing_key);
            let resolved = if configured.is_absolute() {
                configured.to_path_buf()
            } else {
                request.repository_path.join(configured)
            };
            if !resolved.is_file() {
                return Err(AppError::UserActionRequired(
                    "SSH signing key is unavailable".to_owned(),
                ));
            }
        }
        Ok(())
    }
}
impl IdentityProfile {
    pub fn new(display_name: &str, user_name: &str, user_email: &str) -> Self {
        let t = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            display_name: display_name.into(),
            user_name: user_name.into(),
            user_email: user_email.into(),
            gpg_format: None,
            signing_key: None,
            sign_commits: false,
            sign_tags: false,
            created_at: t,
            updated_at: t,
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Theme {
    pub theme_id: String,
    pub plugin_id: String,
    pub version: String,
    pub definition_json: String,
    pub is_valid: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlobalSettings {
    pub global_identity_profile_id: Option<String>,
    pub active_theme_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl Theme {
    pub fn new(id: &str, plugin: &str, version: &str, definition: &str) -> Self {
        let t = Utc::now();
        Self {
            theme_id: id.into(),
            plugin_id: plugin.into(),
            version: version.into(),
            definition_json: definition.into(),
            is_valid: true,
            created_at: t,
            updated_at: t,
        }
    }
}
fn dt(s: String) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::parse_from_rfc3339(&s)
        .map(|x| x.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
fn map_identity(r: &Row) -> Result<IdentityProfile, rusqlite::Error> {
    Ok(IdentityProfile {
        id: r.get(0)?,
        display_name: r.get(1)?,
        user_name: r.get(2)?,
        user_email: r.get(3)?,
        gpg_format: r.get(4)?,
        signing_key: r.get(5)?,
        sign_commits: r.get(6)?,
        sign_tags: r.get(7)?,
        created_at: dt(r.get(8)?)?,
        updated_at: dt(r.get(9)?)?,
    })
}
#[derive(Clone)]
pub struct IdentityProfileRepository {
    db: Database,
}
impl IdentityProfileRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn create(&self, p: &IdentityProfile) -> Result<(), AppError> {
        self.db.with_connection(|c|c.execute("INSERT INTO identity_profiles(id,display_name,user_name,user_email,gpg_format,signing_key,sign_commits,sign_tags,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![p.id,p.display_name,p.user_name,p.user_email,p.gpg_format,p.signing_key,p.sign_commits,p.sign_tags,p.created_at.to_rfc3339(),p.updated_at.to_rfc3339()]).map(|_|())).map_err(|e| map_constraint_error(e, "identity profile"))
    }
    pub fn get(&self, id: &str) -> Result<IdentityProfile, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,display_name,user_name,user_email,gpg_format,signing_key,sign_commits,sign_tags,created_at,updated_at FROM identity_profiles WHERE id=?1",[id],map_identity).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("identity profile {id}"))))
    }

    pub fn list(&self) -> Result<Vec<IdentityProfile>, AppError> {
        self.db.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id,display_name,user_name,user_email,gpg_format,signing_key,sign_commits,sign_tags,created_at,updated_at \
                 FROM identity_profiles ORDER BY display_name,id",
            )?;
            statement
                .query_map([], map_identity)
                .map(|rows| rows.collect())?
        })
    }

    pub fn update(&self, profile: &IdentityProfile) -> Result<(), AppError> {
        let changed = self.db.with_connection(|connection| {
            connection.execute(
                "UPDATE identity_profiles SET display_name=?2,user_name=?3,user_email=?4,gpg_format=?5,signing_key=?6,sign_commits=?7,sign_tags=?8,updated_at=?9 WHERE id=?1",
                params![
                    profile.id,
                    profile.display_name,
                    profile.user_name,
                    profile.user_email,
                    profile.gpg_format,
                    profile.signing_key,
                    profile.sign_commits,
                    profile.sign_tags,
                    profile.updated_at.to_rfc3339()
                ],
            )
        })?;
        if changed == 0 {
            return Err(AppError::NotFound(format!(
                "identity profile {}",
                profile.id
            )));
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let changed = self
            .db
            .with_connection(|connection| {
                connection.execute("DELETE FROM identity_profiles WHERE id=?1", [id])
            })
            .map_err(|error| map_constraint_error(error, "identity profile"))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("identity profile {id}")));
        }
        Ok(())
    }
}
fn map_theme(r: &Row) -> Result<Theme, rusqlite::Error> {
    Ok(Theme {
        theme_id: r.get(0)?,
        plugin_id: r.get(1)?,
        version: r.get(2)?,
        definition_json: r.get(3)?,
        is_valid: r.get(4)?,
        created_at: dt(r.get(5)?)?,
        updated_at: dt(r.get(6)?)?,
    })
}
#[derive(Clone)]
pub struct ThemeRepository {
    db: Database,
}
impl ThemeRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn create(&self, t: &Theme) -> Result<(), AppError> {
        self.db.with_connection(|c|c.execute("INSERT INTO themes(theme_id,plugin_id,version,definition_json,is_valid,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![t.theme_id,t.plugin_id,t.version,t.definition_json,t.is_valid,t.created_at.to_rfc3339(),t.updated_at.to_rfc3339()]).map(|_|())).map_err(|e| map_constraint_error(e, "theme"))
    }
    pub fn get(&self, id: &str) -> Result<Theme, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT theme_id,plugin_id,version,definition_json,is_valid,created_at,updated_at FROM themes WHERE theme_id=?1",[id],map_theme).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("theme {id}"))))
    }
}

#[derive(Clone)]
pub struct GlobalSettingsRepository {
    db: Database,
}
impl GlobalSettingsRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn get(&self) -> Result<GlobalSettings, AppError> {
        self.db.with_connection(|c| {
            c.query_row(
                "SELECT global_identity_profile_id,active_theme_id,created_at,updated_at FROM global_settings WHERE id=1",
                [],
                |r| {
                    Ok(GlobalSettings {
                        global_identity_profile_id: r.get(0)?,
                        active_theme_id: r.get(1)?,
                        created_at: dt(r.get(2)?)?,
                        updated_at: dt(r.get(3)?)?,
                    })
                },
            )
        })
    }
    pub fn set(&self, settings: &GlobalSettings) -> Result<(), AppError> {
        let changed = self.db.with_transaction(|tx| tx.execute("UPDATE global_settings SET global_identity_profile_id=?1,active_theme_id=?2,updated_at=?3 WHERE id=1", params![settings.global_identity_profile_id, settings.active_theme_id, settings.updated_at.to_rfc3339()])).map_err(|e| map_constraint_error(e, "global settings"))?;
        if changed == 0 {
            return Err(AppError::NotFound("global settings".to_owned()));
        }
        Ok(())
    }
}

const IDENTITY_GIT_TIMEOUT: Duration = Duration::from_secs(10);
const MANAGED_CONFIG_KEYS: [&str; 6] = [
    "user.name",
    "user.email",
    "gpg.format",
    "user.signingKey",
    "commit.gpgSign",
    "tag.gpgSign",
];

type ConfigSnapshot = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone)]
enum GlobalConfigTarget {
    Global { working_directory: PathBuf },
    File(PathBuf),
}

impl GlobalConfigTarget {
    fn working_directory(&self) -> PathBuf {
        match self {
            Self::Global { working_directory } => working_directory.clone(),
            Self::File(path) => path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        }
    }

    fn append_scope_args(&self, args: &mut Vec<OsString>) {
        match self {
            Self::Global { .. } => args.push(OsString::from("--global")),
            Self::File(path) => {
                args.push(OsString::from("--file"));
                args.push(path.as_os_str().to_owned());
            }
        }
    }
}

#[derive(Debug, Clone)]
enum ConfigTarget {
    Global(GlobalConfigTarget),
    Local(PathBuf),
}

impl ConfigTarget {
    fn working_directory(&self) -> PathBuf {
        match self {
            Self::Global(target) => target.working_directory(),
            Self::Local(repository) => repository.clone(),
        }
    }

    fn append_scope_args(&self, args: &mut Vec<OsString>) {
        match self {
            Self::Global(target) => target.append_scope_args(args),
            Self::Local(_) => args.push(OsString::from("--local")),
        }
    }
}

/// Owns identity-profile validation, persistence, and the exact Git configuration keys managed
/// by Git-Ramus. Tests can target an explicit config file so no developer Global config is read
/// or written.
#[derive(Clone)]
pub struct IdentityService {
    db: Database,
    runner: Arc<dyn GitRunner>,
    profiles: IdentityProfileRepository,
    settings: GlobalSettingsRepository,
    bindings: IdentityBindingRepository,
    repositories: RepositoryRepository,
    trusts: TrustRepository,
    global_target: GlobalConfigTarget,
    write_locks: RepositoryWriteLocks,
    global_lock: Arc<Mutex<()>>,
    signing_locator: Arc<dyn SigningToolLocator>,
}

impl IdentityService {
    pub fn new(db: Database) -> Self {
        Self::with_write_locks(db, RepositoryWriteLocks::default())
    }

    pub fn with_write_locks(db: Database, write_locks: RepositoryWriteLocks) -> Self {
        let working_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::with_runner_and_target(
            db,
            Arc::new(SystemGitRunner::default()),
            GlobalConfigTarget::Global { working_directory },
            write_locks,
            Arc::new(PathSigningToolLocator),
        )
    }

    pub fn with_runner_and_global_file(
        db: Database,
        runner: Arc<dyn GitRunner>,
        global_file: PathBuf,
    ) -> Result<Self, AppError> {
        if global_file.as_os_str().is_empty() {
            return Err(AppError::InvalidInput(
                "global Git config path is empty".to_owned(),
            ));
        }
        let parent = global_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            return Err(AppError::InvalidInput(
                "global Git config parent must be a directory".to_owned(),
            ));
        }
        Ok(Self::with_runner_and_target(
            db,
            runner,
            GlobalConfigTarget::File(global_file),
            RepositoryWriteLocks::default(),
            Arc::new(PathSigningToolLocator),
        ))
    }

    pub fn with_runner_global_file_and_write_locks(
        db: Database,
        runner: Arc<dyn GitRunner>,
        global_file: PathBuf,
        write_locks: RepositoryWriteLocks,
    ) -> Result<Self, AppError> {
        if global_file.as_os_str().is_empty() {
            return Err(AppError::InvalidInput(
                "global Git config path is empty".to_owned(),
            ));
        }
        let parent = global_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            return Err(AppError::InvalidInput(
                "global Git config parent must be a directory".to_owned(),
            ));
        }
        Ok(Self::with_runner_and_target(
            db,
            runner,
            GlobalConfigTarget::File(global_file),
            write_locks,
            Arc::new(PathSigningToolLocator),
        ))
    }

    pub fn with_runner_global_file_locks_and_signing_locator(
        db: Database,
        runner: Arc<dyn GitRunner>,
        global_file: PathBuf,
        write_locks: RepositoryWriteLocks,
        signing_locator: Arc<dyn SigningToolLocator>,
    ) -> Result<Self, AppError> {
        if global_file.as_os_str().is_empty() {
            return Err(AppError::InvalidInput(
                "global Git config path is empty".to_owned(),
            ));
        }
        let parent = global_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            return Err(AppError::InvalidInput(
                "global Git config parent must be a directory".to_owned(),
            ));
        }
        Ok(Self::with_runner_and_target(
            db,
            runner,
            GlobalConfigTarget::File(global_file),
            write_locks,
            signing_locator,
        ))
    }

    fn with_runner_and_target(
        db: Database,
        runner: Arc<dyn GitRunner>,
        global_target: GlobalConfigTarget,
        write_locks: RepositoryWriteLocks,
        signing_locator: Arc<dyn SigningToolLocator>,
    ) -> Self {
        Self {
            profiles: IdentityProfileRepository::new(db.clone()),
            settings: GlobalSettingsRepository::new(db.clone()),
            bindings: IdentityBindingRepository::new(db.clone()),
            repositories: RepositoryRepository::new(db.clone()),
            trusts: TrustRepository::new(db.clone()),
            db,
            runner,
            global_target,
            write_locks,
            global_lock: Arc::new(Mutex::new(())),
            signing_locator,
        }
    }

    pub fn list(&self) -> Result<Vec<IdentityProfile>, AppError> {
        self.profiles.list()
    }

    pub fn create(&self, input: IdentityProfileInput) -> Result<IdentityProfile, AppError> {
        let normalized = validate_profile_input(input)?;
        let mut profile = IdentityProfile::new(
            &normalized.display_name,
            &normalized.user_name,
            &normalized.user_email,
        );
        profile.gpg_format = normalized.gpg_format;
        profile.signing_key = normalized.signing_key;
        profile.sign_commits = normalized.sign_commits;
        profile.sign_tags = normalized.sign_tags;
        self.profiles.create(&profile)?;
        Ok(profile)
    }

    pub fn update(
        &self,
        profile_id: &str,
        input: IdentityProfileInput,
    ) -> Result<IdentityProfile, AppError> {
        let normalized = validate_profile_input(input)?;
        let _global_guard = self.lock_global()?;
        let mut profile = self.profiles.get(profile_id)?;
        let managed_fields_changed = profile.user_name != normalized.user_name
            || profile.user_email != normalized.user_email
            || profile.gpg_format != normalized.gpg_format
            || profile.signing_key != normalized.signing_key
            || profile.sign_commits != normalized.sign_commits
            || profile.sign_tags != normalized.sign_tags;
        if managed_fields_changed && self.bindings.count_for_profile(profile_id)? > 0 {
            return Err(AppError::UserActionRequired(
                "cleanly unbind all repositories before changing managed identity fields"
                    .to_owned(),
            ));
        }
        profile.display_name = normalized.display_name;
        profile.user_name = normalized.user_name;
        profile.user_email = normalized.user_email;
        profile.gpg_format = normalized.gpg_format;
        profile.signing_key = normalized.signing_key;
        profile.sign_commits = normalized.sign_commits;
        profile.sign_tags = normalized.sign_tags;
        profile.updated_at = Utc::now();

        if managed_fields_changed && self.global_profile_id()?.as_deref() == Some(profile_id) {
            let snapshot = self.apply_global_profile(&profile)?;
            if let Err(error) = self.profiles.update(&profile) {
                self.restore_global_or_error(&snapshot)?;
                return Err(error);
            }
        } else {
            self.profiles.update(&profile)?;
        }
        Ok(profile)
    }

    pub fn delete(&self, profile_id: &str) -> Result<(), AppError> {
        let _global_guard = self.lock_global()?;
        if self.global_profile_id()?.as_deref() == Some(profile_id) {
            return Err(AppError::InvalidInput(
                "current global identity profile must be moved before deletion".to_owned(),
            ));
        }
        self.profiles.delete(profile_id)
    }

    pub fn global_profile_id(&self) -> Result<Option<String>, AppError> {
        Ok(self.settings.get()?.global_identity_profile_id)
    }

    pub fn set_global(&self, profile_id: &str) -> Result<IdentityProfile, AppError> {
        let _global_guard = self.lock_global()?;
        let profile = self.profiles.get(profile_id)?;
        let snapshot = self.apply_global_profile(&profile)?;
        let persisted = (|| {
            let mut settings = self.settings.get()?;
            settings.global_identity_profile_id = Some(profile.id.clone());
            settings.updated_at = Utc::now();
            self.settings.set(&settings)
        })();
        if let Err(error) = persisted {
            self.restore_global_or_error(&snapshot)?;
            return Err(error);
        }
        Ok(profile)
    }

    pub fn bind_repository(
        &self,
        repository_id: &str,
        profile_id: &str,
    ) -> Result<IdentityBinding, AppError> {
        let repository = self.repositories.get(repository_id)?;
        self.require_repository_trust(repository_id)?;
        let _profile_guard = self.lock_global()?;
        let lock = self.local_lock(repository_id);
        let _guard = lock
            .lock()
            .map_err(|_| AppError::Git("identity configuration lock failed".to_owned()))?;
        let profile = self.profiles.get(profile_id)?;
        let target = ConfigTarget::Local(PathBuf::from(&repository.canonical_path));
        if let Some(existing) = self.bindings.get_optional(repository_id)? {
            let current_profile = self.profiles.get(&existing.identity_profile_id)?;
            let actual = self.read_snapshot(&target)?;
            if !config_drift(&profile_config(&current_profile), &actual)
                .fields
                .is_empty()
            {
                return Err(AppError::UserActionRequired(
                    "repository identity configuration changed outside Git-Ramus".to_owned(),
                ));
            }
        }
        let snapshot = self.apply_profile_to_target(&target, &profile)?;
        if let Err(error) = self.bindings.bind(repository_id, profile_id) {
            self.restore_target_or_error(&target, &snapshot)?;
            return Err(error);
        }
        self.bindings.get(repository_id)
    }

    pub fn unbind_repository(&self, repository_id: &str) -> Result<(), AppError> {
        let repository = self.repositories.get(repository_id)?;
        self.require_repository_trust(repository_id)?;
        let _profile_guard = self.lock_global()?;
        let lock = self.local_lock(repository_id);
        let _guard = lock
            .lock()
            .map_err(|_| AppError::Git("identity configuration lock failed".to_owned()))?;
        let Some(binding) = self.bindings.get_optional(repository_id)? else {
            return Ok(());
        };
        let profile = self.profiles.get(&binding.identity_profile_id)?;
        let target = ConfigTarget::Local(PathBuf::from(&repository.canonical_path));
        let actual = self.read_snapshot(&target)?;
        let drift = config_drift(&profile_config(&profile), &actual);
        if !drift.fields.is_empty() {
            return Err(AppError::UserActionRequired(
                "repository identity configuration changed outside Git-Ramus".to_owned(),
            ));
        }
        let empty = empty_config_snapshot();
        if let Err(error) = self.write_snapshot(&target, &empty) {
            self.restore_target_or_error(&target, &actual)?;
            return Err(error);
        }
        if let Err(error) = self.bindings.unbind(repository_id) {
            self.restore_target_or_error(&target, &actual)?;
            return Err(error);
        }
        Ok(())
    }

    pub fn effective_for_repository(
        &self,
        repository_id: &str,
    ) -> Result<EffectiveIdentity, AppError> {
        let repository = self.repositories.get(repository_id)?;
        let repository_path = PathBuf::from(&repository.canonical_path);
        let local_target = ConfigTarget::Local(repository_path.clone());
        let local = self.read_snapshot(&local_target)?;
        if let Some(binding) = self.bindings.get_optional(repository_id)? {
            let profile = self.profiles.get(&binding.identity_profile_id)?;
            let drift = config_drift(&profile_config(&profile), &local);
            return Ok(effective_from_profile(
                repository_id,
                profile,
                IdentitySource::RepositoryProfile,
                (!drift.fields.is_empty()).then_some(drift),
            ));
        }

        let global_profile = self
            .global_profile_id()?
            .map(|profile_id| self.profiles.get(&profile_id))
            .transpose()?;
        let global = self.read_effective_global_snapshot(Some(&repository_path))?;
        let has_local_override = MANAGED_CONFIG_KEYS
            .iter()
            .any(|key| config_key_is_present(&local, key));
        if has_local_override {
            let external_user_name = last_non_empty(global.get("user.name"));
            let external_user_email = last_non_empty(global.get("user.email"));
            let inherited_user_name = global_profile
                .as_ref()
                .map(|profile| profile.user_name.as_str())
                .or(external_user_name.as_deref());
            let inherited_user_email = global_profile
                .as_ref()
                .map(|profile| profile.user_email.as_str())
                .or(external_user_email.as_deref());
            let external_gpg_format = last_non_empty(global.get("gpg.format"));
            let external_signing_key = last_non_empty(global.get("user.signingKey"));
            let inherited_gpg_format = match &global_profile {
                Some(profile) => profile.gpg_format.as_deref(),
                None => external_gpg_format.as_deref(),
            };
            let inherited_signing_key = match &global_profile {
                Some(profile) => profile.signing_key.as_deref(),
                None => external_signing_key.as_deref(),
            };
            let user_name =
                effective_required_text(&local, "user.name", inherited_user_name, "Git user name")?;
            let user_email = effective_required_text(
                &local,
                "user.email",
                inherited_user_email,
                "Git user email",
            )?;
            let gpg_format = effective_optional_text(&local, "gpg.format", inherited_gpg_format);
            let signing_key =
                effective_optional_text(&local, "user.signingKey", inherited_signing_key);
            let sign_commits = effective_bool_with_global(
                &local,
                "commit.gpgSign",
                global_profile.as_ref().map(|profile| profile.sign_commits),
                &global,
            )?;
            let sign_tags = effective_bool_with_global(
                &local,
                "tag.gpgSign",
                global_profile.as_ref().map(|profile| profile.sign_tags),
                &global,
            )?;
            let drift = if let Some(profile) = &global_profile {
                let mut drift = global_config_drift(&profile_config(profile), &global);
                drift
                    .fields
                    .retain(|field| !config_key_is_present(&local, &field.key));
                (!drift.fields.is_empty()).then_some(drift)
            } else {
                None
            };
            return Ok(EffectiveIdentity {
                repository_id: repository_id.to_owned(),
                profile_id: None,
                profile: None,
                source: IdentitySource::ExternalLocal,
                display_name: user_name.clone(),
                user_name,
                user_email,
                gpg_format,
                signing_key,
                sign_commits,
                sign_tags,
                drift,
            });
        }

        if let Some(profile) = global_profile {
            let drift = global_config_drift(&profile_config(&profile), &global);
            Ok(effective_from_profile(
                repository_id,
                profile,
                IdentitySource::GlobalProfile,
                (!drift.fields.is_empty()).then_some(drift),
            ))
        } else {
            effective_from_external_global(repository_id, &global)
        }
    }

    pub fn resolve_commit_identity(
        &self,
        repository_id: &str,
        requested_profile_id: Option<&str>,
    ) -> Result<CommitIdentity, AppError> {
        let repository = self.repositories.get(repository_id)?;
        let effective = if let Some(profile_id) = requested_profile_id {
            let profile = self.profiles.get(profile_id)?;
            effective_from_profile(
                repository_id,
                profile,
                IdentitySource::SelectedProfile,
                None,
            )
        } else {
            self.effective_for_repository(repository_id)?
        };
        if effective.drift.is_some() {
            return Err(AppError::UserActionRequired(
                "Git identity configuration changed outside Git-Ramus".to_owned(),
            ));
        }
        if effective.sign_commits {
            let format = effective.gpg_format.as_deref().ok_or_else(|| {
                AppError::UserActionRequired("signed Commit needs a signing format".to_owned())
            })?;
            if !matches!(format, "openpgp" | "ssh" | "x509") {
                return Err(AppError::UserActionRequired(
                    "signed Commit has an unsupported signing format".to_owned(),
                ));
            }
            let signing_key = effective.signing_key.as_deref().ok_or_else(|| {
                AppError::UserActionRequired("signed Commit needs a signing key".to_owned())
            })?;
            let program = self
                .configured_signing_program(&repository.canonical_path, format)?
                .unwrap_or_else(|| default_signing_program(format).to_owned());
            self.signing_locator.ensure_available(&SigningToolRequest {
                format: format.to_owned(),
                program,
                signing_key: signing_key.to_owned(),
                repository_path: PathBuf::from(&repository.canonical_path),
            })?;
        }
        Ok(CommitIdentity {
            profile_id: effective.profile_id,
            user_name: effective.user_name,
            user_email: effective.user_email,
            gpg_format: effective.gpg_format,
            signing_key: effective.signing_key,
            sign_commits: effective.sign_commits,
        })
    }

    fn configured_signing_program(
        &self,
        repository_path: &str,
        format: &str,
    ) -> Result<Option<String>, AppError> {
        let keys: &[&str] = match format {
            "openpgp" => &["gpg.openpgp.program", "gpg.program"],
            "ssh" => &["gpg.ssh.program"],
            "x509" => &["gpg.x509.program"],
            _ => {
                return Err(AppError::InvalidInput(
                    "unsupported signing format".to_owned(),
                ));
            }
        };
        for key in keys {
            let values = self.read_effective_values(repository_path, key)?;
            if let Some(value) = last_non_empty(Some(&values)) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn read_effective_values(
        &self,
        repository_path: &str,
        key: &str,
    ) -> Result<Vec<String>, AppError> {
        let output = self.runner.run(GitCommand {
            repo: PathBuf::from(repository_path),
            args: vec![
                OsString::from("--no-pager"),
                OsString::from("config"),
                OsString::from("--null"),
                OsString::from("--get-all"),
                OsString::from(key),
            ],
            stdin: None,
            timeout: IDENTITY_GIT_TIMEOUT,
        })?;
        if !output.status.success() {
            if output.status.code() == Some(1) && output.stdout.is_empty() {
                return Ok(Vec::new());
            }
            return Err(config_command_error(&output));
        }
        parse_config_values(&output.stdout)
    }

    /// Imports the developer's existing Global identity only when the database has no profile.
    /// The read target is explicit in tests, which prevents accidental interaction with the
    /// developer's real configuration.
    pub fn import_global_if_empty(&self) -> Result<Option<IdentityProfile>, AppError> {
        let _global_guard = self.lock_global()?;
        if !self.profiles.list()?.is_empty() {
            return Ok(None);
        }
        let snapshot = self.read_effective_global_snapshot(None)?;
        let Some(user_name) = last_non_empty(snapshot.get("user.name")) else {
            return Ok(None);
        };
        let Some(user_email) = last_non_empty(snapshot.get("user.email")) else {
            return Ok(None);
        };
        let mut input = IdentityProfileInput {
            display_name: user_name.clone(),
            user_name,
            user_email,
            gpg_format: last_non_empty(snapshot.get("gpg.format")),
            signing_key: last_non_empty(snapshot.get("user.signingKey")),
            sign_commits: last_bool(snapshot.get("commit.gpgSign")),
            sign_tags: last_bool(snapshot.get("tag.gpgSign")),
        };
        // Git permits implicit OpenPGP format/default-key signing. The stricter Git-Ramus
        // Profile contract cannot safely promise such a key is usable, so import identity data
        // without enabling signing. The actual Global `true` value remains untouched and is
        // surfaced immediately as drift/UserActionRequired.
        let implicit_signing = (input.sign_commits || input.sign_tags)
            && (input.gpg_format.is_none() || input.signing_key.is_none());
        if implicit_signing || (input.gpg_format.is_none() && input.signing_key.is_some()) {
            if input.gpg_format.is_none() && input.signing_key.is_some() {
                input.gpg_format = Some("openpgp".to_owned());
            }
            input.sign_commits = false;
            input.sign_tags = false;
        }
        let normalized = validate_profile_input(input)?;
        let mut profile = IdentityProfile::new(
            &normalized.display_name,
            &normalized.user_name,
            &normalized.user_email,
        );
        profile.gpg_format = normalized.gpg_format;
        profile.signing_key = normalized.signing_key;
        profile.sign_commits = normalized.sign_commits;
        profile.sign_tags = normalized.sign_tags;

        self.db
            .with_transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO identity_profiles(id,display_name,user_name,user_email,gpg_format,signing_key,sign_commits,sign_tags,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![
                        profile.id,
                        profile.display_name,
                        profile.user_name,
                        profile.user_email,
                        profile.gpg_format,
                        profile.signing_key,
                        profile.sign_commits,
                        profile.sign_tags,
                        profile.created_at.to_rfc3339(),
                        profile.updated_at.to_rfc3339()
                    ],
                )?;
                let changed = transaction.execute(
                    "UPDATE global_settings SET global_identity_profile_id=?1,updated_at=?2 WHERE id=1",
                    params![profile.id, Utc::now().to_rfc3339()],
                )?;
                if changed != 1 {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
                Ok(())
            })
            .map_err(|error| map_constraint_error(error, "identity profile import"))?;
        Ok(Some(profile))
    }

    fn apply_global_profile(&self, profile: &IdentityProfile) -> Result<ConfigSnapshot, AppError> {
        let target = ConfigTarget::Global(self.global_target.clone());
        let snapshot = self.read_writable_global_snapshot()?;
        if let Err(error) = self.write_snapshot(&target, &profile_config(profile)) {
            self.restore_target_or_error(&target, &snapshot)?;
            return Err(error);
        }
        Ok(snapshot)
    }

    fn restore_global_or_error(&self, snapshot: &ConfigSnapshot) -> Result<(), AppError> {
        self.restore_target_or_error(&ConfigTarget::Global(self.global_target.clone()), snapshot)
    }

    fn read_writable_global_snapshot(&self) -> Result<ConfigSnapshot, AppError> {
        self.read_snapshot(&ConfigTarget::Global(self.global_target.clone()))
    }

    fn read_effective_global_snapshot(
        &self,
        repository: Option<&Path>,
    ) -> Result<ConfigSnapshot, AppError> {
        let working_directory = repository
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.global_target.working_directory());
        let mut snapshot = BTreeMap::new();
        for key in MANAGED_CONFIG_KEYS {
            let values = self.read_effective_global_values(&working_directory, key)?;
            snapshot.insert(key.to_owned(), values.last().cloned().into_iter().collect());
        }
        Ok(snapshot)
    }

    fn read_effective_global_values(
        &self,
        working_directory: &Path,
        key: &str,
    ) -> Result<Vec<String>, AppError> {
        let mut args = vec![OsString::from("--no-pager"), OsString::from("config")];
        self.global_target.append_scope_args(&mut args);
        args.extend([
            OsString::from("--includes"),
            OsString::from("--null"),
            OsString::from("--get-all"),
            OsString::from(key),
        ]);
        let output = self.runner.run(GitCommand {
            repo: working_directory.to_path_buf(),
            args,
            stdin: None,
            timeout: IDENTITY_GIT_TIMEOUT,
        })?;
        if !output.status.success() {
            if output.status.code() == Some(1) && output.stdout.is_empty() {
                return Ok(Vec::new());
            }
            return Err(config_command_error(&output));
        }
        parse_config_values(&output.stdout)
    }

    fn apply_profile_to_target(
        &self,
        target: &ConfigTarget,
        profile: &IdentityProfile,
    ) -> Result<ConfigSnapshot, AppError> {
        let snapshot = self.read_snapshot(target)?;
        if let Err(error) = self.write_snapshot(target, &profile_config(profile)) {
            self.restore_target_or_error(target, &snapshot)?;
            return Err(error);
        }
        Ok(snapshot)
    }

    fn restore_target_or_error(
        &self,
        target: &ConfigTarget,
        snapshot: &ConfigSnapshot,
    ) -> Result<(), AppError> {
        self.write_snapshot(target, snapshot)
            .map_err(|_| AppError::Git("identity configuration rollback failed".to_owned()))
    }

    fn read_snapshot(&self, target: &ConfigTarget) -> Result<ConfigSnapshot, AppError> {
        let mut snapshot = BTreeMap::new();
        for key in MANAGED_CONFIG_KEYS {
            snapshot.insert(key.to_owned(), self.read_values(target, key)?);
        }
        Ok(snapshot)
    }

    fn write_snapshot(
        &self,
        target: &ConfigTarget,
        snapshot: &ConfigSnapshot,
    ) -> Result<(), AppError> {
        for key in MANAGED_CONFIG_KEYS {
            let values = snapshot.get(key).map(Vec::as_slice).unwrap_or(&[]);
            self.write_values(target, key, values)?;
            let actual = self.read_values(target, key)?;
            if actual != values {
                return Err(AppError::Git(
                    "Git identity configuration readback mismatch".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn read_values(&self, target: &ConfigTarget, key: &str) -> Result<Vec<String>, AppError> {
        let mut args = vec![OsString::from("--no-pager"), OsString::from("config")];
        target.append_scope_args(&mut args);
        args.extend([
            OsString::from("--null"),
            OsString::from("--get-all"),
            OsString::from(key),
        ]);
        let output = self.run_config_git(target, args)?;
        if !output.status.success() {
            if output.status.code() == Some(1) && output.stdout.is_empty() {
                return Ok(Vec::new());
            }
            return Err(config_command_error(&output));
        }
        parse_config_values(&output.stdout)
    }

    fn write_values(
        &self,
        target: &ConfigTarget,
        key: &str,
        values: &[String],
    ) -> Result<(), AppError> {
        let mut unset = vec![OsString::from("--no-pager"), OsString::from("config")];
        target.append_scope_args(&mut unset);
        unset.extend([OsString::from("--unset-all"), OsString::from(key)]);
        let output = self.run_config_git(target, unset)?;
        if !output.status.success() && !matches!(output.status.code(), Some(1 | 5)) {
            return Err(config_command_error(&output));
        }
        for value in values {
            let mut add = vec![OsString::from("--no-pager"), OsString::from("config")];
            target.append_scope_args(&mut add);
            add.extend([
                OsString::from("--add"),
                OsString::from(key),
                OsString::from(value),
            ]);
            let output = self.run_config_git(target, add)?;
            if !output.status.success() {
                return Err(config_command_error(&output));
            }
        }
        Ok(())
    }

    fn run_config_git(
        &self,
        target: &ConfigTarget,
        args: Vec<OsString>,
    ) -> Result<GitOutput, AppError> {
        self.runner.run(GitCommand {
            repo: target.working_directory(),
            args,
            stdin: None,
            timeout: IDENTITY_GIT_TIMEOUT,
        })
    }

    fn require_repository_trust(&self, repository_id: &str) -> Result<(), AppError> {
        if self.trusts.is_trusted(repository_id)? {
            Ok(())
        } else {
            Err(AppError::TrustRequired)
        }
    }

    fn local_lock(&self, repository_id: &str) -> Arc<Mutex<()>> {
        self.write_locks.lock_for(repository_id)
    }

    fn lock_global(&self) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        self.global_lock
            .lock()
            .map_err(|_| AppError::Git("global identity configuration lock failed".to_owned()))
    }
}

fn validate_profile_input(input: IdentityProfileInput) -> Result<IdentityProfileInput, AppError> {
    let display_name = bounded_identity_text(&input.display_name, "display name", 256)?;
    let user_name = bounded_identity_text(&input.user_name, "Git user name", 256)?;
    let user_email = bounded_identity_text(&input.user_email, "Git user email", 320)?;
    if !valid_email_shape(&user_email) {
        return Err(AppError::InvalidInput(
            "Git user email has an invalid shape".to_owned(),
        ));
    }
    let gpg_format = match input.gpg_format.as_deref().map(str::trim) {
        None | Some("none") => None,
        Some(value @ ("openpgp" | "ssh" | "x509")) => Some(value.to_owned()),
        Some(_) => {
            return Err(AppError::InvalidInput(
                "gpg format must be openpgp, ssh, x509, or none".to_owned(),
            ));
        }
    };
    let signing_key = match input.signing_key.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(value) if value.len() <= 4 * 1024 && !value.contains('\0') => Some(value.to_owned()),
        Some(_) => {
            return Err(AppError::InvalidInput("signing key is invalid".to_owned()));
        }
    };
    if gpg_format.is_none() && signing_key.is_some() {
        return Err(AppError::InvalidInput(
            "signing key requires a signing format".to_owned(),
        ));
    }
    if (input.sign_commits || input.sign_tags) && (gpg_format.is_none() || signing_key.is_none()) {
        return Err(AppError::InvalidInput(
            "enabled signing requires a format and signing key".to_owned(),
        ));
    }
    Ok(IdentityProfileInput {
        display_name,
        user_name,
        user_email,
        gpg_format,
        signing_key,
        sign_commits: input.sign_commits,
        sign_tags: input.sign_tags,
    })
}

fn bounded_identity_text(value: &str, label: &str, max: usize) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max || value.contains('\0') {
        return Err(AppError::InvalidInput(format!(
            "{label} must contain 1 to {max} characters"
        )));
    }
    Ok(value.to_owned())
}

fn valid_email_shape(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) || value.chars().any(char::is_control) {
        return false;
    }
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn profile_config(profile: &IdentityProfile) -> ConfigSnapshot {
    let mut config = BTreeMap::new();
    config.insert("user.name".to_owned(), vec![profile.user_name.clone()]);
    config.insert("user.email".to_owned(), vec![profile.user_email.clone()]);
    config.insert(
        "gpg.format".to_owned(),
        profile.gpg_format.iter().cloned().collect(),
    );
    config.insert(
        "user.signingKey".to_owned(),
        profile.signing_key.iter().cloned().collect(),
    );
    config.insert(
        "commit.gpgSign".to_owned(),
        vec![profile.sign_commits.to_string()],
    );
    config.insert(
        "tag.gpgSign".to_owned(),
        vec![profile.sign_tags.to_string()],
    );
    config
}

fn empty_config_snapshot() -> ConfigSnapshot {
    MANAGED_CONFIG_KEYS
        .into_iter()
        .map(|key| (key.to_owned(), Vec::new()))
        .collect()
}

fn config_key_is_present(snapshot: &ConfigSnapshot, key: &str) -> bool {
    snapshot.get(key).is_some_and(|values| !values.is_empty())
}

fn effective_required_text(
    local: &ConfigSnapshot,
    key: &str,
    inherited: Option<&str>,
    label: &str,
) -> Result<String, AppError> {
    let value = if config_key_is_present(local, key) {
        local
            .get(key)
            .and_then(|values| values.last())
            .map(String::as_str)
    } else {
        inherited
    };
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| AppError::UserActionRequired(format!("{label} is not configured")))
}

fn effective_optional_text(
    local: &ConfigSnapshot,
    key: &str,
    inherited: Option<&str>,
) -> Option<String> {
    if config_key_is_present(local, key) {
        last_non_empty(local.get(key))
    } else {
        inherited.map(str::to_owned)
    }
}

fn effective_bool_with_global(
    local: &ConfigSnapshot,
    key: &str,
    profile_value: Option<bool>,
    global: &ConfigSnapshot,
) -> Result<bool, AppError> {
    if !config_key_is_present(local, key) {
        if let Some(value) = profile_value {
            return Ok(value);
        }
        return semantic_bool(global.get(key)).map_err(|()| {
            AppError::UserActionRequired(format!("Git configuration {key} is invalid"))
        });
    }
    last_optional_bool(local.get(key))
        .ok_or_else(|| AppError::UserActionRequired(format!("Git configuration {key} is invalid")))
}

fn config_drift(expected: &ConfigSnapshot, actual: &ConfigSnapshot) -> IdentityDrift {
    let fields = MANAGED_CONFIG_KEYS
        .into_iter()
        .filter_map(|key| {
            let expected = expected.get(key).cloned().unwrap_or_default();
            let actual = actual.get(key).cloned().unwrap_or_default();
            (expected != actual).then(|| IdentityDriftField {
                key: key.to_owned(),
                expected,
                actual,
            })
        })
        .collect();
    IdentityDrift { fields }
}

fn global_config_drift(expected: &ConfigSnapshot, actual: &ConfigSnapshot) -> IdentityDrift {
    let fields = MANAGED_CONFIG_KEYS
        .into_iter()
        .filter_map(|key| {
            let expected_values = expected.get(key).cloned().unwrap_or_default();
            let actual_values = actual.get(key).cloned().unwrap_or_default();
            let expected_semantic = expected_values
                .last()
                .cloned()
                .into_iter()
                .collect::<Vec<_>>();
            let actual_semantic = actual_values
                .last()
                .cloned()
                .into_iter()
                .collect::<Vec<_>>();
            let equivalent = if matches!(key, "commit.gpgSign" | "tag.gpgSign") {
                matches!(
                    (semantic_bool(Some(&expected_values)), semantic_bool(Some(&actual_values))),
                    (Ok(expected), Ok(actual)) if expected == actual
                )
            } else {
                expected_semantic == actual_semantic
            };
            (!equivalent).then(|| IdentityDriftField {
                key: key.to_owned(),
                expected: expected_semantic,
                actual: actual_semantic,
            })
        })
        .collect();
    IdentityDrift { fields }
}

fn semantic_bool(values: Option<&Vec<String>>) -> Result<bool, ()> {
    if values.is_none_or(Vec::is_empty) {
        Ok(false)
    } else {
        last_optional_bool(values).ok_or(())
    }
}

fn effective_from_profile(
    repository_id: &str,
    profile: IdentityProfile,
    source: IdentitySource,
    drift: Option<IdentityDrift>,
) -> EffectiveIdentity {
    EffectiveIdentity {
        repository_id: repository_id.to_owned(),
        profile_id: Some(profile.id.clone()),
        profile: Some(profile.clone()),
        source,
        display_name: profile.display_name,
        user_name: profile.user_name,
        user_email: profile.user_email,
        gpg_format: profile.gpg_format,
        signing_key: profile.signing_key,
        sign_commits: profile.sign_commits,
        sign_tags: profile.sign_tags,
        drift,
    }
}

fn effective_from_external_global(
    repository_id: &str,
    global: &ConfigSnapshot,
) -> Result<EffectiveIdentity, AppError> {
    let empty = empty_config_snapshot();
    let user_name = effective_required_text(global, "user.name", None, "Git user name")?;
    let user_email = effective_required_text(global, "user.email", None, "Git user email")?;
    let gpg_format = effective_optional_text(global, "gpg.format", None);
    let signing_key = effective_optional_text(global, "user.signingKey", None);
    let sign_commits = effective_bool_with_global(global, "commit.gpgSign", None, &empty)?;
    let sign_tags = effective_bool_with_global(global, "tag.gpgSign", None, &empty)?;
    Ok(EffectiveIdentity {
        repository_id: repository_id.to_owned(),
        profile_id: None,
        profile: None,
        source: IdentitySource::ExternalGlobal,
        display_name: user_name.clone(),
        user_name,
        user_email,
        gpg_format,
        signing_key,
        sign_commits,
        sign_tags,
        drift: None,
    })
}

fn append_config_override(args: &mut Vec<OsString>, key: &str, value: &str) {
    args.push(OsString::from("-c"));
    args.push(OsString::from(format!("{key}={value}")));
}

fn default_signing_program(format: &str) -> &'static str {
    match format {
        "ssh" => "ssh-keygen",
        "x509" => "gpgsm",
        _ => "gpg",
    }
}

fn program_exists(program: &str, working_directory: &Path) -> bool {
    if program.trim().is_empty() || program.contains('\0') {
        return false;
    }
    let path = Path::new(program);
    if path.is_absolute() {
        return is_executable_file(path);
    }
    if path.components().count() > 1 {
        return is_executable_file(&working_directory.join(path));
    }
    let Some(search_path) = std::env::var_os("PATH") else {
        return false;
    };
    let extensions = executable_extensions(path);
    std::env::split_paths(&search_path).any(|directory| {
        extensions
            .iter()
            .any(|extension| is_executable_file(&directory.join(format!("{program}{extension}"))))
    })
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return false;
        };
        let extension = format!(".{extension}");
        let path_ext = std::env::var_os("PATHEXT")
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
        path_ext
            .split(';')
            .any(|candidate| candidate.eq_ignore_ascii_case(&extension))
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

fn executable_extensions(program: &Path) -> Vec<String> {
    if program.extension().is_some() {
        return vec![String::new()];
    }
    #[cfg(windows)]
    {
        let mut extensions = vec![String::new()];
        if let Some(path_ext) = std::env::var_os("PATHEXT") {
            extensions.extend(
                path_ext
                    .to_string_lossy()
                    .split(';')
                    .filter(|extension| !extension.is_empty())
                    .map(str::to_owned),
            );
        } else {
            extensions.extend([".COM", ".EXE", ".BAT", ".CMD"].map(str::to_owned));
        }
        extensions
    }
    #[cfg(not(windows))]
    {
        vec![String::new()]
    }
}

fn parse_config_values(bytes: &[u8]) -> Result<Vec<String>, AppError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(AppError::Git(
            "Git identity configuration output was malformed".to_owned(),
        ));
    }
    bytes[..bytes.len() - 1]
        .split(|byte| *byte == 0)
        .map(|value| {
            std::str::from_utf8(value).map(str::to_owned).map_err(|_| {
                AppError::Git("Git identity configuration output was malformed".to_owned())
            })
        })
        .collect()
}

fn config_command_error(output: &GitOutput) -> AppError {
    let _ = output;
    AppError::Git("Git identity configuration failed".to_owned())
}

fn last_non_empty(values: Option<&Vec<String>>) -> Option<String> {
    values
        .and_then(|values| values.last())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn last_bool(values: Option<&Vec<String>>) -> bool {
    values
        .and_then(|values| values.last())
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "true" | "yes" | "on" | "1"
            )
        })
}

fn last_optional_bool(values: Option<&Vec<String>>) -> Option<bool> {
    let value = values?.last()?.trim().to_ascii_lowercase();
    match value.as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    #[test]
    fn global_settings_is_singleton() {
        let db = Database::open_in_memory().unwrap();
        let count: i64 = db
            .with_connection(|c| {
                c.query_row("SELECT COUNT(*) FROM global_settings", [], |r| r.get(0))
            })
            .unwrap();
        assert_eq!(count, 1);
        let identities = super::IdentityProfileRepository::new(db.clone());
        let profile = super::IdentityProfile::new("A", "a", "a@example.com");
        identities.create(&profile).unwrap();
        let themes = super::ThemeRepository::new(db.clone());
        let theme = super::Theme::new("theme", "builtin", "1", "{}");
        themes.create(&theme).unwrap();
        let settings_repo = super::GlobalSettingsRepository::new(db);
        let mut settings = settings_repo.get().unwrap();
        settings.global_identity_profile_id = Some(profile.id);
        settings.active_theme_id = Some(theme.theme_id);
        settings.updated_at = chrono::Utc::now();
        settings_repo.set(&settings).unwrap();
        let loaded = settings_repo.get().unwrap();
        assert_eq!(
            loaded.global_identity_profile_id,
            settings.global_identity_profile_id
        );
        assert_eq!(loaded.active_theme_id, settings.active_theme_id);
    }
}

#[cfg(test)]
mod service_tests {
    use std::path::Path;
    use std::process::Command;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    };
    use std::time::Duration;

    use crate::db::Database;
    use crate::error::AppError;
    use crate::git::engine::{GitCommand, GitOutput, GitRunner, SystemGitRunner};
    use crate::git::model::{Repository, RepositoryKind, Trust};
    use crate::git::repository::{
        IdentityBindingRepository, RepositoryRepository, RepositoryWriteLocks, TrustRepository,
    };

    use super::{IdentityProfileInput, IdentityService, IdentitySource};

    #[derive(Debug, Clone, Copy)]
    enum ConfigFault {
        Write,
        Readback,
    }

    struct FaultInjectingRunner {
        inner: SystemGitRunner,
        key: &'static str,
        fault: ConfigFault,
        armed: AtomicBool,
        fired: AtomicBool,
    }

    impl FaultInjectingRunner {
        fn new(key: &'static str, fault: ConfigFault) -> Self {
            Self {
                inner: SystemGitRunner::default(),
                key,
                fault,
                armed: AtomicBool::new(false),
                fired: AtomicBool::new(false),
            }
        }

        fn matches(args: &[String], operation: &str, key: &str) -> bool {
            args.iter().any(|arg| arg == operation) && args.iter().any(|arg| arg == key)
        }
    }

    impl GitRunner for FaultInjectingRunner {
        fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
            let args = command
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            if matches!(self.fault, ConfigFault::Write)
                && Self::matches(&args, "--add", self.key)
                && !self.fired.swap(true, Ordering::AcqRel)
            {
                return Err(AppError::Git("injected config write failure".to_owned()));
            }
            let mut output = self.inner.run(command)?;
            if matches!(self.fault, ConfigFault::Readback) {
                if Self::matches(&args, "--add", self.key) {
                    self.armed.store(true, Ordering::Release);
                } else if Self::matches(&args, "--get-all", self.key)
                    && self.armed.swap(false, Ordering::AcqRel)
                    && !self.fired.swap(true, Ordering::AcqRel)
                {
                    output.stdout = b"injected-mismatch\0".to_vec();
                }
            }
            Ok(output)
        }
    }

    struct BlockingBindRunner {
        inner: SystemGitRunner,
        blocked: AtomicBool,
        entered: Mutex<Option<mpsc::Sender<()>>>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl BlockingBindRunner {
        fn new() -> (Self, mpsc::Receiver<()>, mpsc::Sender<()>) {
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            (
                Self {
                    inner: SystemGitRunner::default(),
                    blocked: AtomicBool::new(false),
                    entered: Mutex::new(Some(entered_tx)),
                    release: Mutex::new(release_rx),
                },
                entered_rx,
                release_tx,
            )
        }
    }

    impl GitRunner for BlockingBindRunner {
        fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
            let args = command
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            if FaultInjectingRunner::matches(&args, "--add", "user.name")
                && args.iter().any(|arg| arg == "--local")
                && !self.blocked.swap(true, Ordering::AcqRel)
            {
                if let Some(sender) = self
                    .entered
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    let _ = sender.send(());
                }
                self.release
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| AppError::Git("blocking test release timed out".to_owned()))?;
            }
            self.inner.run(command)
        }
    }

    fn run_git_config(file: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["config", "--file"])
            .arg(file)
            .args(args)
            .output()
            .expect("git config starts");
        assert!(
            output.status.success(),
            "git config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git config output is UTF-8")
            .trim()
            .to_owned()
    }

    fn run_git_config_at(repository: &Path, file: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(repository)
            .args(["config", "--file"])
            .arg(file)
            .args(args)
            .output()
            .expect("repository-aware git config starts");
        assert!(
            output.status.success(),
            "repository-aware git config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git config output is UTF-8")
            .trim()
            .to_owned()
    }

    fn conditional_include_section(repository: &Path, included_file_name: &str) -> String {
        let mut git_dir = std::fs::canonicalize(repository.join(".git"))
            .expect("Git directory canonicalizes")
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(without_verbatim_prefix) = git_dir.strip_prefix("//?/") {
            git_dir = without_verbatim_prefix.to_owned();
        }
        format!("[includeIf \"gitdir/i:{git_dir}\"]\n\tpath = {included_file_name}\n")
    }

    fn input(display_name: &str, user_name: &str, user_email: &str) -> IdentityProfileInput {
        IdentityProfileInput {
            display_name: display_name.to_owned(),
            user_name: user_name.to_owned(),
            user_email: user_email.to_owned(),
            gpg_format: None,
            signing_key: None,
            sign_commits: false,
            sign_tags: false,
        }
    }

    fn isolated_service(directory: &Path) -> IdentityService {
        isolated_service_with_db(directory).1
    }

    fn isolated_service_with_db(directory: &Path) -> (Database, IdentityService) {
        let database = Database::open_in_memory().expect("database opens");
        let service = IdentityService::with_runner_and_global_file(
            database.clone(),
            Arc::new(SystemGitRunner::default()),
            directory.join("global.gitconfig"),
        )
        .expect("identity service constructs");
        (database, service)
    }

    #[test]
    fn identity_service_uses_the_injected_repository_write_lock_registry() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let database = Database::open_in_memory().expect("database opens");
        let locks = RepositoryWriteLocks::default();
        let service = IdentityService::with_runner_global_file_and_write_locks(
            database,
            Arc::new(SystemGitRunner::default()),
            directory.path().join("global.gitconfig"),
            locks.clone(),
        )
        .expect("identity service constructs");

        assert!(Arc::ptr_eq(
            &locks.lock_for("repository"),
            &service.local_lock("repository")
        ));
    }

    fn init_repository(path: &Path) {
        std::fs::create_dir_all(path).expect("repository directory creates");
        let output = Command::new("git")
            .current_dir(path)
            .args(["init", "--quiet"])
            .output()
            .expect("git init starts");
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn register_repository(database: &Database, path: &Path, trusted: bool) -> Repository {
        let canonical = std::fs::canonicalize(path).expect("repository path canonicalizes");
        let repository = Repository::new(
            canonical.to_str().expect("path is UTF-8"),
            "fixture",
            RepositoryKind::Normal,
        );
        RepositoryRepository::new(database.clone())
            .create(&repository)
            .expect("repository persists");
        if trusted {
            TrustRepository::new(database.clone())
                .set(&Trust {
                    repository_id: repository.id.clone(),
                    trusted_at: chrono::Utc::now(),
                    trust_version: 1,
                })
                .expect("repository trust persists");
        }
        repository
    }

    fn run_local_config(repository: &Path, args: &[&str]) -> Option<String> {
        let output = Command::new("git")
            .current_dir(repository)
            .args(["config", "--local"])
            .args(args)
            .output()
            .expect("local git config starts");
        if output.status.code() == Some(1) && output.stdout.is_empty() {
            return None;
        }
        assert!(
            output.status.success(),
            "local git config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Some(
            String::from_utf8(output.stdout)
                .expect("local config output is UTF-8")
                .trim()
                .to_owned(),
        )
    }

    #[test]
    fn import_global_if_empty_creates_the_first_profile_and_pointer() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        run_git_config(&config, &["user.name", "Imported User"]);
        run_git_config(&config, &["user.email", "imported@example.com"]);
        let service = isolated_service(directory.path());

        let imported = service
            .import_global_if_empty()
            .expect("global import succeeds")
            .expect("profile is imported");

        assert_eq!(imported.display_name, "Imported User");
        assert_eq!(imported.user_name, "Imported User");
        assert_eq!(imported.user_email, "imported@example.com");
        assert_eq!(
            service.list().expect("profiles list"),
            vec![imported.clone()]
        );
        assert_eq!(
            service.global_profile_id().expect("global pointer reads"),
            Some(imported.id)
        );
        assert!(
            service
                .import_global_if_empty()
                .expect("second import succeeds")
                .is_none()
        );
    }

    #[test]
    fn unconditional_global_include_is_imported_without_mutating_either_file() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let main = directory.path().join("global.gitconfig");
        let included = directory.path().join("included.gitconfig");
        std::fs::write(
            &included,
            "[user]\n\tname = Included User\n\temail = included@example.com\n",
        )
        .expect("included config writes");
        std::fs::write(&main, "[include]\n\tpath = included.gitconfig\n")
            .expect("main config writes");
        let main_before = std::fs::read(&main).expect("main config reads");
        let included_before = std::fs::read(&included).expect("included config reads");
        let service = isolated_service(directory.path());

        let imported = service
            .import_global_if_empty()
            .expect("include-aware import succeeds")
            .expect("included identity imports");

        assert_eq!(imported.user_name, "Included User");
        assert_eq!(imported.user_email, "included@example.com");
        assert_eq!(std::fs::read(&main).unwrap(), main_before);
        assert_eq!(std::fs::read(&included).unwrap(), included_before);
    }

    #[test]
    fn imported_absent_false_signing_flags_do_not_report_immediate_global_drift() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        run_git_config(&config, &["user.name", "Imported User"]);
        run_git_config(&config, &["user.email", "imported@example.com"]);
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        service
            .import_global_if_empty()
            .expect("global import succeeds")
            .expect("profile imports");

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");

        assert_eq!(effective.source, IdentitySource::GlobalProfile);
        assert_eq!(effective.drift, None);
    }

    #[test]
    fn malformed_present_global_boolean_is_reported_as_profile_drift() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let profile = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("profile creates");
        service.set_global(&profile.id).expect("global applies");
        run_git_config(&config, &["commit.gpgSign", "definitely-not-a-bool"]);

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves with drift");

        assert!(
            effective
                .drift
                .as_ref()
                .expect("malformed present boolean is drift")
                .fields
                .iter()
                .any(|field| field.key == "commit.gpgSign"
                    && field.actual == vec!["definitely-not-a-bool"])
        );
        assert!(matches!(
            service.resolve_commit_identity(&repository.id, None),
            Err(AppError::UserActionRequired(_))
        ));
    }

    #[test]
    fn matching_include_if_resolves_external_global_identity_without_a_database_profile() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let main = directory.path().join("global.gitconfig");
        let conditional = directory.path().join("conditional.gitconfig");
        std::fs::write(
            &conditional,
            "[user]\n\tname = Conditional User\n\temail = conditional@example.com\n",
        )
        .expect("conditional config writes");
        std::fs::write(
            &main,
            conditional_include_section(&repository_path, "conditional.gitconfig"),
        )
        .expect("main config writes");
        assert_eq!(
            run_git_config_at(
                &repository_path,
                &main,
                &["--includes", "--get", "user.name"]
            ),
            "Conditional User"
        );
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("conditional external identity resolves");

        assert_eq!(effective.profile_id, None);
        assert_eq!(effective.user_name, "Conditional User");
        assert_eq!(effective.user_email, "conditional@example.com");
        assert!(effective.drift.is_none());
    }

    #[test]
    fn conditional_global_override_is_reported_as_profile_drift() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let main = directory.path().join("global.gitconfig");
        let conditional = directory.path().join("conditional.gitconfig");
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let profile = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("profile creates");
        service.set_global(&profile.id).expect("global applies");
        std::fs::write(&conditional, "[user]\n\temail = conditional@example.com\n")
            .expect("conditional config writes");
        use std::io::Write as _;
        let mut main_file = std::fs::OpenOptions::new()
            .append(true)
            .open(&main)
            .expect("main config opens");
        main_file
            .write_all(
                conditional_include_section(&repository_path, "conditional.gitconfig").as_bytes(),
            )
            .expect("conditional include appends");
        drop(main_file);
        assert_eq!(
            run_git_config_at(
                &repository_path,
                &main,
                &["--includes", "--get", "user.email"]
            ),
            "conditional@example.com"
        );

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");

        assert!(
            effective
                .drift
                .as_ref()
                .expect("conditional override is drift")
                .fields
                .iter()
                .any(|field| field.key == "user.email"
                    && field.actual == vec!["conditional@example.com"])
        );
        assert!(matches!(
            service.resolve_commit_identity(&repository.id, None),
            Err(AppError::UserActionRequired(_))
        ));
    }

    #[test]
    fn import_global_with_implicit_signing_defaults_is_nonfatal_and_requires_action() {
        for (format, key) in [
            (None, None),
            (Some("openpgp"), None),
            (None, Some("implicit-key-id")),
        ] {
            let directory = tempfile::tempdir().expect("temporary directory creates");
            let config = directory.path().join("global.gitconfig");
            run_git_config(&config, &["user.name", "Imported User"]);
            run_git_config(&config, &["user.email", "imported@example.com"]);
            run_git_config(&config, &["commit.gpgSign", "true"]);
            if let Some(format) = format {
                run_git_config(&config, &["gpg.format", format]);
            }
            if let Some(key) = key {
                run_git_config(&config, &["user.signingKey", key]);
            }
            let repository_path = directory.path().join("repo");
            init_repository(&repository_path);
            let (database, service) = isolated_service_with_db(directory.path());
            let repository = register_repository(&database, &repository_path, false);

            let imported = service
                .import_global_if_empty()
                .expect("implicit signing config must not fail startup import")
                .expect("profile imports");
            let effective = service
                .effective_for_repository(&repository.id)
                .expect("effective identity resolves");

            assert!(!imported.sign_commits, "unsafe implicit signing was stored");
            assert!(
                effective
                    .drift
                    .as_ref()
                    .expect("implicit signing is surfaced as drift")
                    .fields
                    .iter()
                    .any(|field| field.key == "commit.gpgSign")
            );
            assert!(matches!(
                service.resolve_commit_identity(&repository.id, None),
                Err(AppError::UserActionRequired(_))
            ));
        }
    }

    #[test]
    fn profile_validation_rejects_invalid_fields_and_normalizes_none_format() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let service = isolated_service(directory.path());

        for invalid in [
            input("", "Alice", "alice@example.com"),
            input("Alice", "", "alice@example.com"),
            input("Alice", "Alice", "not-an-email"),
        ] {
            assert!(matches!(
                service.create(invalid),
                Err(AppError::InvalidInput(_))
            ));
        }
        let mut unsupported = input("Alice", "Alice", "alice@example.com");
        unsupported.gpg_format = Some("smime".to_owned());
        assert!(matches!(
            service.create(unsupported),
            Err(AppError::InvalidInput(_))
        ));
        let mut missing_key = input("Alice", "Alice", "alice@example.com");
        missing_key.gpg_format = Some("ssh".to_owned());
        missing_key.sign_commits = true;
        assert!(matches!(
            service.create(missing_key),
            Err(AppError::InvalidInput(_))
        ));

        let mut no_signing = input("Alice", "Alice", "alice@example.com");
        no_signing.gpg_format = Some("none".to_owned());
        let created = service.create(no_signing).expect("none format is accepted");
        assert_eq!(created.gpg_format, None);
        assert_eq!(created.signing_key, None);
    }

    #[test]
    fn moving_global_pointer_applies_each_profile_to_isolated_git_config() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let service = isolated_service(directory.path());
        let first = service
            .create(input("Work", "Work User", "work@example.com"))
            .expect("first profile creates");
        let second = service
            .create(input("Personal", "Personal User", "me@example.com"))
            .expect("second profile creates");

        service.set_global(&first.id).expect("first global applies");
        assert_eq!(service.global_profile_id().unwrap(), Some(first.id));
        assert_eq!(
            run_git_config(
                &directory.path().join("global.gitconfig"),
                &["--get", "user.email"]
            ),
            "work@example.com"
        );

        service
            .set_global(&second.id)
            .expect("second global applies");
        assert_eq!(service.global_profile_id().unwrap(), Some(second.id));
        assert_eq!(
            run_git_config(
                &directory.path().join("global.gitconfig"),
                &["--get", "user.name"]
            ),
            "Personal User"
        );
    }

    #[test]
    fn setting_global_profile_with_include_changes_only_the_main_config() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        let included = directory.path().join("included.gitconfig");
        std::fs::write(
            &included,
            "[user]\n\tname = Included User\n\temail = included@example.com\n",
        )
        .expect("included config writes");
        std::fs::write(&config, "[include]\n\tpath = included.gitconfig\n")
            .expect("main config writes");
        let included_before = std::fs::read(&included).expect("included config reads");
        let service = isolated_service(directory.path());
        let profile = service
            .create(input("Global", "Managed User", "managed@example.com"))
            .expect("profile creates");

        service.set_global(&profile.id).expect("global applies");

        assert_eq!(
            run_git_config(&config, &["--get", "user.name"]),
            "Managed User"
        );
        assert_eq!(std::fs::read(&included).unwrap(), included_before);
    }

    #[test]
    fn deleting_the_current_global_profile_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let service = isolated_service(directory.path());
        let profile = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("profile creates");
        service.set_global(&profile.id).expect("global applies");

        assert!(matches!(
            service.delete(&profile.id),
            Err(AppError::InvalidInput(message)) if message.contains("global")
        ));
        assert_eq!(service.list().expect("profiles list"), vec![profile]);
    }

    #[test]
    fn updating_the_current_global_profile_reapplies_git_config() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let service = isolated_service(directory.path());
        let profile = service
            .create(input("Before", "Before User", "before@example.com"))
            .expect("profile creates");
        service.set_global(&profile.id).expect("global applies");

        let updated = service
            .update(
                &profile.id,
                input("After", "After User", "after@example.com"),
            )
            .expect("global profile updates");

        assert_eq!(updated.id, profile.id);
        assert_eq!(updated.created_at, profile.created_at);
        assert_eq!(
            run_git_config(
                &directory.path().join("global.gitconfig"),
                &["--get", "user.name"]
            ),
            "After User"
        );
        assert_eq!(service.list().expect("profiles list"), vec![updated]);
    }

    #[test]
    fn global_display_only_update_preserves_external_git_drift() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let profile = service
            .create(input("Before Label", "Global User", "global@example.com"))
            .expect("profile creates");
        service.set_global(&profile.id).expect("global applies");
        run_git_config(&config, &["user.email", "outside@example.com"]);

        let updated = service
            .update(
                &profile.id,
                input("After Label", "Global User", "global@example.com"),
            )
            .expect("display-only update succeeds");
        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");

        assert_eq!(updated.display_name, "After Label");
        assert_eq!(
            run_git_config(&config, &["--get", "user.email"]),
            "outside@example.com"
        );
        assert!(
            effective
                .drift
                .as_ref()
                .expect("external drift remains")
                .fields
                .iter()
                .any(|field| field.key == "user.email"
                    && field.actual == vec!["outside@example.com"])
        );
    }

    #[test]
    fn binding_repository_supports_optional_lookup_and_unbind() {
        let database = Database::open_in_memory().expect("database opens");
        let profiles = super::IdentityProfileRepository::new(database.clone());
        let profile = super::IdentityProfile::new("Profile", "User", "user@example.com");
        profiles.create(&profile).expect("profile creates");
        let repositories = RepositoryRepository::new(database.clone());
        let repository = Repository::new("C:/repo", "repo", RepositoryKind::Normal);
        repositories
            .create(&repository)
            .expect("repository creates");
        let bindings = IdentityBindingRepository::new(database);

        assert_eq!(
            bindings
                .get_optional(&repository.id)
                .expect("optional binding reads"),
            None
        );
        bindings
            .bind(&repository.id, &profile.id)
            .expect("binding creates");
        assert_eq!(
            bindings
                .get_optional(&repository.id)
                .expect("binding reads")
                .expect("binding exists")
                .identity_profile_id,
            profile.id
        );
        bindings.unbind(&repository.id).expect("binding removes");
        assert_eq!(bindings.get_optional(&repository.id).unwrap(), None);
    }

    #[test]
    fn binding_a_trusted_repository_applies_local_config_and_reports_profile_source() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let profile = service
            .create(input("Work", "Work User", "work@example.com"))
            .expect("profile creates");

        let binding = service
            .bind_repository(&repository.id, &profile.id)
            .expect("trusted repository binds");

        assert_eq!(binding.identity_profile_id, profile.id);
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.name"]).as_deref(),
            Some("Work User")
        );
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.email"]).as_deref(),
            Some("work@example.com")
        );
        assert_eq!(
            run_local_config(&repository_path, &["--get", "commit.gpgSign"]).as_deref(),
            Some("false")
        );
        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");
        assert_eq!(effective.source, IdentitySource::RepositoryProfile);
        assert_eq!(effective.profile_id.as_deref(), Some(profile.id.as_str()));
        assert!(effective.drift.is_none());
    }

    #[test]
    fn binding_an_untrusted_repository_does_not_touch_git_or_database() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let profile = service
            .create(input("Work", "Work User", "work@example.com"))
            .expect("profile creates");

        assert!(matches!(
            service.bind_repository(&repository.id, &profile.id),
            Err(AppError::TrustRequired)
        ));
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.name"]),
            None
        );
        assert_eq!(
            IdentityBindingRepository::new(database)
                .get_optional(&repository.id)
                .expect("binding lookup succeeds"),
            None
        );
    }

    #[test]
    fn external_local_drift_is_reported_and_follow_global_refuses_to_overwrite_it() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let profile = service
            .create(input("Work", "Work User", "work@example.com"))
            .expect("profile creates");
        service
            .bind_repository(&repository.id, &profile.id)
            .expect("repository binds");
        run_local_config(&repository_path, &["user.email", "outside@example.com"]);

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");
        let drift = effective.drift.expect("drift is reported");
        assert!(drift.fields.iter().any(|field| field.key == "user.email"
            && field.expected == vec!["work@example.com"]
            && field.actual == vec!["outside@example.com"]));
        assert!(matches!(
            service.unbind_repository(&repository.id),
            Err(AppError::UserActionRequired(_))
        ));
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.email"]).as_deref(),
            Some("outside@example.com")
        );
        assert!(
            IdentityBindingRepository::new(database)
                .get_optional(&repository.id)
                .expect("binding lookup succeeds")
                .is_some()
        );
    }

    #[test]
    fn rebind_refuses_existing_drift_but_clean_rebind_can_switch_profiles() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let first = service
            .create(input("First", "First User", "first@example.com"))
            .expect("first profile creates");
        let second = service
            .create(input("Second", "Second User", "second@example.com"))
            .expect("second profile creates");
        service
            .bind_repository(&repository.id, &first.id)
            .expect("first profile binds");
        run_local_config(&repository_path, &["user.email", "outside@example.com"]);

        assert!(matches!(
            service.bind_repository(&repository.id, &second.id),
            Err(AppError::UserActionRequired(_))
        ));
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.email"]).as_deref(),
            Some("outside@example.com")
        );
        assert_eq!(
            IdentityBindingRepository::new(database.clone())
                .get(&repository.id)
                .expect("binding remains")
                .identity_profile_id,
            first.id
        );

        run_local_config(&repository_path, &["user.email", "first@example.com"]);
        service
            .bind_repository(&repository.id, &second.id)
            .expect("clean rebind succeeds");
        assert_eq!(
            IdentityBindingRepository::new(database)
                .get(&repository.id)
                .expect("binding switches")
                .identity_profile_id,
            second.id
        );
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.email"]).as_deref(),
            Some("second@example.com")
        );
    }

    #[test]
    fn bound_profile_managed_update_is_rejected_without_database_or_git_changes() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let profile = service
            .create(input("Profile", "Original User", "original@example.com"))
            .expect("profile creates");
        service
            .bind_repository(&repository.id, &profile.id)
            .expect("profile binds");

        assert!(matches!(
            service.update(
                &profile.id,
                input("Profile", "Changed User", "changed@example.com")
            ),
            Err(AppError::UserActionRequired(message)) if message.contains("unbind")
        ));
        let stored = service
            .list()
            .expect("profiles list")
            .into_iter()
            .find(|candidate| candidate.id == profile.id)
            .expect("profile remains");
        assert_eq!(stored.user_name, "Original User");
        assert_eq!(stored.user_email, "original@example.com");
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.name"]).as_deref(),
            Some("Original User")
        );
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.email"]).as_deref(),
            Some("original@example.com")
        );
    }

    #[test]
    fn bound_profile_display_only_update_is_allowed_and_unbind_then_managed_update_succeeds() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let profile = service
            .create(input(
                "Before Label",
                "Original User",
                "original@example.com",
            ))
            .expect("profile creates");
        service
            .bind_repository(&repository.id, &profile.id)
            .expect("profile binds");

        let display_only = service
            .update(
                &profile.id,
                input("After Label", "Original User", "original@example.com"),
            )
            .expect("display-only update succeeds");
        assert_eq!(display_only.display_name, "After Label");
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.name"]).as_deref(),
            Some("Original User")
        );

        service
            .unbind_repository(&repository.id)
            .expect("clean profile unbinds");
        let managed_update = service
            .update(
                &profile.id,
                input("After Label", "Changed User", "changed@example.com"),
            )
            .expect("managed update succeeds after unbind");
        assert_eq!(managed_update.user_name, "Changed User");
        assert_eq!(managed_update.user_email, "changed@example.com");
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.name"]),
            None
        );
    }

    #[test]
    fn concurrent_bind_serializes_profile_update_without_deadlock_or_stale_local_config() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let database = Database::open_in_memory().expect("database opens");
        let repository = register_repository(&database, &repository_path, true);
        let (runner, bind_entered, release_bind) = BlockingBindRunner::new();
        let service = Arc::new(
            IdentityService::with_runner_and_global_file(
                database,
                Arc::new(runner),
                directory.path().join("global.gitconfig"),
            )
            .expect("identity service constructs"),
        );
        let profile = service
            .create(input("Profile", "Original User", "original@example.com"))
            .expect("profile creates");

        let bind_service = Arc::clone(&service);
        let repository_id = repository.id.clone();
        let profile_id = profile.id.clone();
        let bind =
            std::thread::spawn(move || bind_service.bind_repository(&repository_id, &profile_id));
        bind_entered
            .recv_timeout(Duration::from_secs(5))
            .expect("bind reaches blocked Local write");

        let update_service = Arc::clone(&service);
        let update_profile_id = profile.id.clone();
        let (updated_tx, updated_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = update_service.update(
                &update_profile_id,
                input("Profile", "Changed User", "changed@example.com"),
            );
            let _ = updated_tx.send(result);
        });
        let early = match updated_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(result) => Some(result),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => panic!("update thread disconnected"),
        };
        release_bind.send(()).expect("bind releases");
        bind.join()
            .expect("bind thread joins")
            .expect("bind succeeds");
        let completed_early = early.is_some();
        let update_result = match early {
            Some(result) => result,
            None => updated_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("serialized update completes"),
        };

        assert!(
            !completed_early,
            "profile update escaped the bind lifecycle lock"
        );
        assert!(matches!(
            update_result,
            Err(AppError::UserActionRequired(_))
        ));
        let stored = service
            .list()
            .expect("profiles list")
            .into_iter()
            .find(|candidate| candidate.id == profile.id)
            .expect("profile remains");
        assert_eq!(stored.user_name, "Original User");
        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.name"]).as_deref(),
            Some("Original User")
        );
    }

    #[test]
    fn follow_global_removes_only_managed_local_keys() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let global = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("global profile creates");
        service.set_global(&global.id).expect("global applies");
        let local = service
            .create(input("Local", "Local User", "local@example.com"))
            .expect("local profile creates");
        run_local_config(&repository_path, &["core.editor", "external-editor"]);
        service
            .bind_repository(&repository.id, &local.id)
            .expect("repository binds");

        service
            .unbind_repository(&repository.id)
            .expect("follow global succeeds");

        assert_eq!(
            run_local_config(&repository_path, &["--get", "user.name"]),
            None
        );
        assert_eq!(
            run_local_config(&repository_path, &["--get", "core.editor"]).as_deref(),
            Some("external-editor")
        );
        assert_eq!(
            IdentityBindingRepository::new(database)
                .get_optional(&repository.id)
                .expect("binding lookup succeeds"),
            None
        );
        assert_eq!(
            service
                .effective_for_repository(&repository.id)
                .expect("effective identity resolves")
                .source,
            IdentitySource::GlobalProfile
        );
    }

    #[test]
    fn external_local_name_and_email_win_when_repository_has_no_binding() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let global = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("global profile creates");
        service.set_global(&global.id).expect("global applies");
        run_local_config(&repository_path, &["user.name", "External User"]);
        run_local_config(&repository_path, &["user.email", "external@example.com"]);

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");

        assert_eq!(effective.source, IdentitySource::ExternalLocal);
        assert_eq!(effective.profile_id, None);
        assert_eq!(effective.user_name, "External User");
        assert_eq!(effective.user_email, "external@example.com");
        assert!(effective.drift.is_none());
    }

    #[test]
    fn signing_only_local_overrides_resolve_as_external_without_unsigned_downgrade() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let global = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("global profile creates");
        service.set_global(&global.id).expect("global applies");
        let key = directory.path().join("signing-key");
        std::fs::write(&key, "placeholder").expect("key writes");
        run_local_config(&repository_path, &["gpg.format", "ssh"]);
        run_local_config(
            &repository_path,
            &["user.signingKey", key.to_str().expect("key path is UTF-8")],
        );
        run_local_config(&repository_path, &["commit.gpgSign", "true"]);
        run_local_config(
            &repository_path,
            &[
                "gpg.ssh.program",
                std::env::current_exe()
                    .expect("current executable resolves")
                    .to_str()
                    .expect("current executable is UTF-8"),
            ],
        );

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");
        let commit_identity = service
            .resolve_commit_identity(&repository.id, None)
            .expect("external signed identity resolves");

        assert_eq!(effective.source, IdentitySource::ExternalLocal);
        assert_eq!(effective.user_name, "Global User");
        assert_eq!(effective.user_email, "global@example.com");
        assert_eq!(effective.gpg_format.as_deref(), Some("ssh"));
        assert!(effective.sign_commits);
        assert!(commit_identity.sign_commits);
        assert_eq!(commit_identity.gpg_format.as_deref(), Some("ssh"));
    }

    #[test]
    fn partial_local_override_reports_drift_for_inherited_global_keys() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let global = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("global profile creates");
        service.set_global(&global.id).expect("global applies");
        run_git_config(
            &directory.path().join("global.gitconfig"),
            &["user.email", "outside@example.com"],
        );
        run_local_config(&repository_path, &["commit.gpgSign", "false"]);

        let effective = service
            .effective_for_repository(&repository.id)
            .expect("effective identity resolves");

        assert_eq!(effective.source, IdentitySource::ExternalLocal);
        assert!(
            effective
                .drift
                .as_ref()
                .expect("inherited global drift is reported")
                .fields
                .iter()
                .any(|field| field.key == "user.email"
                    && field.expected == vec!["global@example.com"]
                    && field.actual == vec!["outside@example.com"])
        );
        assert!(matches!(
            service.resolve_commit_identity(&repository.id, None),
            Err(AppError::UserActionRequired(_))
        ));
    }

    #[test]
    fn incomplete_effective_local_signing_configuration_requires_user_action() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, false);
        let global = service
            .create(input("Global", "Global User", "global@example.com"))
            .expect("global profile creates");
        service.set_global(&global.id).expect("global applies");
        run_local_config(&repository_path, &["gpg.format", "ssh"]);
        run_local_config(&repository_path, &["commit.gpgSign", "true"]);

        assert!(matches!(
            service.resolve_commit_identity(&repository.id, None),
            Err(AppError::UserActionRequired(message)) if message.contains("signing key")
        ));
    }

    #[test]
    fn set_global_rolls_git_config_back_when_database_pointer_update_fails() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        run_git_config(&config, &["user.name", "Before"]);
        run_git_config(&config, &["user.email", "before@example.com"]);
        let (database, service) = isolated_service_with_db(directory.path());
        let profile = service
            .create(input("After", "After", "after@example.com"))
            .expect("profile creates");
        database
            .with_connection(|connection| {
                connection
                    .execute("DELETE FROM global_settings WHERE id=1", [])
                    .map(|_| ())
            })
            .expect("global settings row deletes");

        assert!(service.set_global(&profile.id).is_err());
        assert_eq!(run_git_config(&config, &["--get", "user.name"]), "Before");
        assert_eq!(
            run_git_config(&config, &["--get", "user.email"]),
            "before@example.com"
        );
    }

    #[test]
    fn set_global_rolls_all_keys_back_when_a_config_write_fails() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        run_git_config(&config, &["user.name", "Before"]);
        run_git_config(&config, &["user.email", "before@example.com"]);
        let database = Database::open_in_memory().expect("database opens");
        let service = IdentityService::with_runner_and_global_file(
            database,
            Arc::new(FaultInjectingRunner::new("user.email", ConfigFault::Write)),
            config.clone(),
        )
        .expect("identity service constructs");
        let profile = service
            .create(input("After", "After", "after@example.com"))
            .expect("profile creates");

        assert!(matches!(
            service.set_global(&profile.id),
            Err(AppError::Git(_))
        ));
        assert_eq!(run_git_config(&config, &["--get", "user.name"]), "Before");
        assert_eq!(
            run_git_config(&config, &["--get", "user.email"]),
            "before@example.com"
        );
        assert_eq!(service.global_profile_id().unwrap(), None);
    }

    #[test]
    fn global_write_failure_restores_main_config_without_touching_include() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        let included = directory.path().join("included.gitconfig");
        std::fs::write(
            &included,
            "[user]\n\tname = Included User\n\temail = included@example.com\n",
        )
        .expect("included config writes");
        std::fs::write(&config, "[include]\n\tpath = included.gitconfig\n")
            .expect("main config writes");
        run_git_config(&config, &["user.name", "Before"]);
        run_git_config(&config, &["user.email", "before@example.com"]);
        let included_before = std::fs::read(&included).expect("included config reads");
        let database = Database::open_in_memory().expect("database opens");
        let service = IdentityService::with_runner_and_global_file(
            database,
            Arc::new(FaultInjectingRunner::new("user.email", ConfigFault::Write)),
            config.clone(),
        )
        .expect("identity service constructs");
        let profile = service
            .create(input("After", "After", "after@example.com"))
            .expect("profile creates");

        assert!(matches!(
            service.set_global(&profile.id),
            Err(AppError::Git(_))
        ));

        assert_eq!(run_git_config(&config, &["--get", "user.name"]), "Before");
        assert_eq!(
            run_git_config(&config, &["--get", "user.email"]),
            "before@example.com"
        );
        assert_eq!(std::fs::read(&included).unwrap(), included_before);
        assert_eq!(service.global_profile_id().unwrap(), None);
    }

    #[test]
    fn set_global_rolls_all_keys_back_when_readback_mismatches() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let config = directory.path().join("global.gitconfig");
        run_git_config(&config, &["user.name", "Before"]);
        run_git_config(&config, &["user.email", "before@example.com"]);
        let database = Database::open_in_memory().expect("database opens");
        let service = IdentityService::with_runner_and_global_file(
            database,
            Arc::new(FaultInjectingRunner::new(
                "user.email",
                ConfigFault::Readback,
            )),
            config.clone(),
        )
        .expect("identity service constructs");
        let profile = service
            .create(input("After", "After", "after@example.com"))
            .expect("profile creates");

        assert!(matches!(
            service.set_global(&profile.id),
            Err(AppError::Git(_))
        ));
        assert_eq!(run_git_config(&config, &["--get", "user.name"]), "Before");
        assert_eq!(
            run_git_config(&config, &["--get", "user.email"]),
            "before@example.com"
        );
        assert_eq!(service.global_profile_id().unwrap(), None);
    }

    #[test]
    fn configured_signing_program_unavailable_requires_user_action() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let key = directory.path().join("signing-key");
        std::fs::write(&key, "placeholder").expect("placeholder key writes");
        let mut signed = input("Signed", "Signed User", "signed@example.com");
        signed.gpg_format = Some("ssh".to_owned());
        signed.signing_key = Some(key.to_string_lossy().into_owned());
        signed.sign_commits = true;
        let profile = service.create(signed).expect("signed profile creates");
        run_local_config(
            &repository_path,
            &["gpg.ssh.program", "git-ramus-tool-that-does-not-exist"],
        );

        assert!(matches!(
            service.resolve_commit_identity(&repository.id, Some(&profile.id)),
            Err(AppError::UserActionRequired(message)) if message.contains("signing tool")
        ));
    }

    #[test]
    fn unavailable_ssh_signing_key_requires_user_action_before_commit() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let missing_key = directory.path().join("missing-signing-key");
        let mut signed = input("Signed", "Signed User", "signed@example.com");
        signed.gpg_format = Some("ssh".to_owned());
        signed.signing_key = Some(missing_key.to_string_lossy().into_owned());
        signed.sign_commits = true;
        let profile = service.create(signed).expect("signed profile creates");
        run_local_config(
            &repository_path,
            &[
                "gpg.ssh.program",
                std::env::current_exe()
                    .expect("current executable resolves")
                    .to_str()
                    .expect("current executable is UTF-8"),
            ],
        );

        assert!(matches!(
            service.resolve_commit_identity(&repository.id, Some(&profile.id)),
            Err(AppError::UserActionRequired(message)) if message.contains("signing key")
        ));
    }

    #[test]
    fn configured_signing_program_must_be_an_executable_not_just_an_existing_file() {
        let directory = tempfile::tempdir().expect("temporary directory creates");
        let repository_path = directory.path().join("repo");
        init_repository(&repository_path);
        let (database, service) = isolated_service_with_db(directory.path());
        let repository = register_repository(&database, &repository_path, true);
        let key = directory.path().join("signing-key");
        std::fs::write(&key, "placeholder").expect("placeholder key writes");
        let program = directory.path().join("not-executable.txt");
        std::fs::write(&program, "not an executable").expect("plain file writes");
        let mut signed = input("Signed", "Signed User", "signed@example.com");
        signed.gpg_format = Some("ssh".to_owned());
        signed.signing_key = Some(key.to_string_lossy().into_owned());
        signed.sign_commits = true;
        let profile = service.create(signed).expect("signed profile creates");
        run_local_config(
            &repository_path,
            &[
                "gpg.ssh.program",
                program.to_str().expect("program path is UTF-8"),
            ],
        );

        assert!(matches!(
            service.resolve_commit_identity(&repository.id, Some(&profile.id)),
            Err(AppError::UserActionRequired(message)) if message.contains("signing tool")
        ));
    }

    #[test]
    fn profile_input_uses_camel_case_and_rejects_unknown_fields() {
        let parsed: IdentityProfileInput = serde_json::from_value(serde_json::json!({
            "displayName": "Alice",
            "userName": "Alice",
            "userEmail": "alice@example.com",
            "gpgFormat": null,
            "signingKey": null,
            "signCommits": false,
            "signTags": false
        }))
        .expect("camel-case input parses");
        assert_eq!(parsed.user_email, "alice@example.com");
        assert!(
            serde_json::from_value::<IdentityProfileInput>(serde_json::json!({
                "displayName": "Alice",
                "userName": "Alice",
                "userEmail": "alice@example.com",
                "gpgFormat": null,
                "signingKey": null,
                "signCommits": false,
                "signTags": false,
                "unexpected": true
            }))
            .is_err()
        );
    }
}
