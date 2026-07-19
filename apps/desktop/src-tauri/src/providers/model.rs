use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Github,
    Gitlab,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "github" => Ok(Self::Github),
            "gitlab" => Ok(Self::Gitlab),
            _ => Err(AppError::InvalidInput("unknown Provider kind".to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderConnectionStatus {
    Connected,
    ActionRequired,
    RateLimited,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderInstance {
    pub id: String,
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_base_url: String,
    pub custom_ca_path: Option<String>,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub server_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstanceSummary {
    pub id: String,
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub custom_ca_configured: bool,
    pub custom_ca_label: Option<String>,
    pub provider_enabled: bool,
    pub status: ProviderConnectionStatus,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub server_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProviderInstance {
    pub fn summary(
        &self,
        provider_enabled: bool,
        status: ProviderConnectionStatus,
    ) -> ProviderInstanceSummary {
        ProviderInstanceSummary {
            id: self.id.clone(),
            provider_kind: self.provider_kind,
            display_name: self.display_name.clone(),
            base_url: self.base_url.clone(),
            custom_ca_configured: self.custom_ca_path.is_some(),
            custom_ca_label: self.custom_ca_path.as_deref().and_then(|path| {
                Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }),
            provider_enabled,
            status,
            last_validated_at: self.last_validated_at,
            server_version: self.server_version.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct NewProviderAccount {
    pub id: String,
    pub instance_id: String,
    pub provider_user_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub secret_ref: String,
    pub last_validated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderAccount {
    pub id: String,
    pub instance_id: String,
    pub provider_user_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub secret_ref: String,
    pub is_default: bool,
    pub last_validated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountSummary {
    pub id: String,
    pub instance_id: String,
    pub provider_user_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_default: bool,
    pub status: ProviderConnectionStatus,
    pub last_validated_at: DateTime<Utc>,
}

impl ProviderAccount {
    pub fn summary(&self, status: ProviderConnectionStatus) -> ProviderAccountSummary {
        ProviderAccountSummary {
            id: self.id.clone(),
            instance_id: self.instance_id.clone(),
            provider_user_id: self.provider_user_id.clone(),
            username: self.username.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            is_default: self.is_default,
            status,
            last_validated_at: self.last_validated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthorizedAccount {
    pub instance: ProviderInstanceSummary,
    pub account: ProviderAccountSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingSource {
    Auto,
    Manual,
}

impl BindingSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

impl std::str::FromStr for BindingSource {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "manual" => Ok(Self::Manual),
            _ => Err(AppError::InvalidInput(
                "unknown Provider binding source".to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBinding {
    pub repository_id: String,
    pub remote_name: String,
    pub provider_instance_id: String,
    pub provider_account_id: Option<String>,
    pub provider_repository_id: String,
    pub full_name: String,
    pub web_url: String,
    pub matched_url: String,
    pub binding_source: BindingSource,
    pub bound_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderBindingSuggestionStatus {
    Suggested,
    Ambiguous,
    Unverified,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBindingSuggestion {
    pub repository_id: String,
    pub remote_name: String,
    pub instance_id: String,
    pub status: ProviderBindingSuggestionStatus,
    pub provider_repository_id: Option<String>,
    pub full_name: Option<String>,
    pub web_url: Option<String>,
    pub matched_url: Option<String>,
    pub candidates: Vec<RemoteRepository>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RemoteRepositoryIdentity {
    Id { repository_id: String },
    Path { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderVisibility {
    Public,
    Internal,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderPermission {
    Read,
    Write,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderArchivedFilter {
    All,
    Active,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderRepositorySort {
    Name,
    Updated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderRepositoryDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRepositoryQuery {
    pub search: String,
    pub visibility: Option<ProviderVisibility>,
    pub namespace: Option<String>,
    pub archived: ProviderArchivedFilter,
    pub sort: ProviderRepositorySort,
    pub direction: ProviderRepositoryDirection,
    pub page_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRateLimitState {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub reset_at: Option<DateTime<Utc>>,
    pub retry_after_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRepository {
    pub provider_kind: ProviderKind,
    pub instance_id: String,
    pub repository_id: String,
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub web_url: String,
    pub https_url: String,
    pub ssh_url: String,
    pub default_branch: Option<String>,
    pub visibility: ProviderVisibility,
    pub archived: bool,
    pub fork: bool,
    pub permission: ProviderPermission,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRepositoryPage {
    pub items: Vec<RemoteRepository>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub rate_limit: Option<ProviderRateLimitState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterCursor {
    Page(u64),
    Keyset(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceMetadata {
    pub server_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    pub provider_user_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterListRequest {
    pub query: ProviderRepositoryQuery,
    pub cursor: Option<AdapterCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterPage {
    pub items: Vec<RemoteRepository>,
    pub next_cursor: Option<AdapterCursor>,
    pub rate_limit: Option<ProviderRateLimitState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AccountDeletionResolution {
    Reassign { account_id: String },
    Inherit,
    Unbind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDeletionImpact {
    pub account_id: String,
    pub instance_id: String,
    pub is_default: bool,
    pub explicit_binding_count: i64,
    pub inherited_binding_count: i64,
    pub sibling_account_ids: Vec<String>,
    pub requires_new_default: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretCleanupRecord {
    pub secret_ref: String,
    pub created_at: DateTime<Utc>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub attempt_count: i64,
    pub last_error_code: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        AccountDeletionResolution, ProviderConnectionStatus, ProviderInstance, ProviderKind,
    };

    #[test]
    fn account_deletion_resolution_uses_camel_case_fields() {
        let value = serde_json::to_value(AccountDeletionResolution::Reassign {
            account_id: "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50".to_owned(),
        })
        .expect("resolution serializes");
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "reassign",
                "accountId": "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50"
            })
        );
    }

    #[test]
    fn instance_summary_exposes_only_the_custom_ca_file_name() {
        let now = Utc::now();
        let summary = ProviderInstance {
            id: "instance".to_owned(),
            provider_kind: ProviderKind::Gitlab,
            display_name: "GitLab".to_owned(),
            base_url: "https://gitlab.example".to_owned(),
            api_base_url: "https://gitlab.example/api/v4".to_owned(),
            custom_ca_path: Some("/private/certificates/company-root.pem".to_owned()),
            last_validated_at: Some(now),
            server_version: None,
            created_at: now,
            updated_at: now,
        }
        .summary(true, ProviderConnectionStatus::Connected);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(summary.custom_ca_configured);
        assert_eq!(summary.custom_ca_label.as_deref(), Some("company-root.pem"));
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("certificates"));
    }
}
