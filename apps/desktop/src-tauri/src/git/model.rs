use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn now() -> DateTime<Utc> {
    Utc::now()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub root_path: String,
    pub name: String,
    pub scan_depth: i64,
    pub exclude_patterns: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl Project {
    pub fn new(root_path: &str, name: &str) -> Self {
        let t = now();
        Self {
            id: Uuid::new_v4().to_string(),
            root_path: root_path.into(),
            name: name.into(),
            scan_depth: 2,
            exclude_patterns: Vec::new(),
            created_at: t,
            updated_at: t,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl Workspace {
    pub fn new(name: &str) -> Self {
        let t = now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            created_at: t,
            updated_at: t,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RepositoryKind {
    Normal,
    Bare,
    Worktree,
}
impl RepositoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Bare => "bare",
            Self::Worktree => "worktree",
        }
    }
}
impl std::str::FromStr for RepositoryKind {
    type Err = crate::error::AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normal" => Ok(Self::Normal),
            "bare" => Ok(Self::Bare),
            "worktree" => Ok(Self::Worktree),
            _ => Err(crate::error::AppError::InvalidInput(format!(
                "unknown repository kind: {s}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Repository {
    pub id: String,
    pub canonical_path: String,
    pub display_name: String,
    pub kind: RepositoryKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
impl Repository {
    pub fn new(path: &str, name: &str, kind: RepositoryKind) -> Self {
        let t = now();
        Self {
            id: Uuid::new_v4().to_string(),
            canonical_path: path.into(),
            display_name: name.into(),
            kind,
            created_at: t,
            updated_at: t,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepositorySnapshot {
    pub id: String,
    pub repository_id: String,
    pub captured_at: DateTime<Utc>,
    pub head_oid: Option<String>,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
    pub dirty: bool,
    pub staged_count: i64,
    pub unstaged_count: i64,
    pub untracked_count: i64,
    pub conflicted_count: i64,
    pub refresh_error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Remote {
    pub repository_id: String,
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Trust {
    pub repository_id: String,
    pub trusted_at: DateTime<Utc>,
    pub trust_version: i64,
}

pub use crate::identity::{IdentityProfile, Theme};
