#[cfg(test)]
mod roundtrip_tests {
    use crate::db::Database;

    #[test]
    fn identity_and_theme_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let identities = super::IdentityProfileRepository::new(db.clone());
        let themes = super::ThemeRepository::new(db);
        let profile = super::IdentityProfile::new("Alice", "alice", "alice@example.com");
        identities.create(&profile).unwrap();
        assert_eq!(identities.get(&profile.id).unwrap().display_name, "Alice");
        let theme = super::Theme::new("dark", "builtin", "1.0", "{}");
        themes.create(&theme).unwrap();
        assert_eq!(themes.get(&theme.theme_id).unwrap().theme_id, "dark");
    }
}
use crate::{db::Database, error::AppError};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
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
        self.db.with_connection(|c|c.execute("INSERT INTO identity_profiles(id,display_name,user_name,user_email,gpg_format,signing_key,sign_commits,sign_tags,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![p.id,p.display_name,p.user_name,p.user_email,p.gpg_format,p.signing_key,p.sign_commits,p.sign_tags,p.created_at.to_rfc3339(),p.updated_at.to_rfc3339()]).map(|_|())).map_err(AppError::from)
    }
    pub fn get(&self, id: &str) -> Result<IdentityProfile, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,display_name,user_name,user_email,gpg_format,signing_key,sign_commits,sign_tags,created_at,updated_at FROM identity_profiles WHERE id=?1",[id],map_identity).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("identity profile {id}"))))
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
        self.db.with_connection(|c|c.execute("INSERT INTO themes(theme_id,plugin_id,version,definition_json,is_valid,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![t.theme_id,t.plugin_id,t.version,t.definition_json,t.is_valid,t.created_at.to_rfc3339(),t.updated_at.to_rfc3339()]).map(|_|())).map_err(AppError::from)
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
        self.db.with_connection(|c| c.query_row("SELECT global_identity_profile_id,active_theme_id FROM global_settings WHERE id=1", [], |r| Ok(GlobalSettings { global_identity_profile_id: r.get(0)?, active_theme_id: r.get(1)? }))).map_err(AppError::from)
    }
    pub fn set(&self, settings: &GlobalSettings) -> Result<(), AppError> {
        self.db.with_transaction(|tx| tx.execute("UPDATE global_settings SET global_identity_profile_id=?1,active_theme_id=?2 WHERE id=1", params![settings.global_identity_profile_id, settings.active_theme_id]).map(|_| ()))
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
    }
}
