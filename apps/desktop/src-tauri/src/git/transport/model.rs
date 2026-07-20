use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::git::model::RepositorySnapshot;
use crate::git::service::QueryContext;
use crate::jobs::model::Job;
use crate::providers::model::RemoteRepository;

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn portable_file_name(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
        .map(str::to_owned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Ssh,
    Https,
}

impl TransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Https => "https",
        }
    }
}

impl FromStr for TransportKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ssh" => Ok(Self::Ssh),
            "https" => Ok(Self::Https),
            _ => Err(AppError::InvalidInput("unknown transport kind".to_owned())),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct TransportProfile {
    pub id: String,
    pub display_name: String,
    pub kind: TransportKind,
    pub ssh_key_path: Option<String>,
    pub ssh_variant: Option<String>,
    pub ssh_identities_only: Option<bool>,
    pub https_username: Option<String>,
    pub https_use_http_path: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Debug for TransportProfile {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransportProfile")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .field("kind", &self.kind)
            .field(
                "ssh_key_file_name",
                &self.ssh_key_path.as_deref().and_then(portable_file_name),
            )
            .field("ssh_variant", &self.ssh_variant)
            .field("ssh_identities_only", &self.ssh_identities_only)
            .field("https_username", &self.https_username)
            .field("https_use_http_path", &self.https_use_http_path)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl TransportProfile {
    pub fn new_ssh(
        display_name: impl Into<String>,
        key_path: impl Into<String>,
        identities_only: bool,
    ) -> Self {
        let timestamp = now();
        Self {
            id: Uuid::new_v4().to_string(),
            display_name: display_name.into(),
            kind: TransportKind::Ssh,
            ssh_key_path: Some(key_path.into()),
            ssh_variant: Some("ssh".to_owned()),
            ssh_identities_only: Some(identities_only),
            https_username: None,
            https_use_http_path: None,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    pub fn new_https(display_name: impl Into<String>, username: impl Into<String>) -> Self {
        let timestamp = now();
        Self {
            id: Uuid::new_v4().to_string(),
            display_name: display_name.into(),
            kind: TransportKind::Https,
            ssh_key_path: None,
            ssh_variant: None,
            ssh_identities_only: None,
            https_username: Some(username.into()),
            https_use_http_path: Some(true),
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    pub fn summary(&self, available: bool, bound_repository_count: i64) -> TransportProfileSummary {
        TransportProfileSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            kind: self.kind,
            ssh_key_file_name: self.ssh_key_path.as_deref().and_then(portable_file_name),
            https_username: self.https_username.clone(),
            available,
            bound_repository_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransportProfileSummary {
    pub id: String,
    pub display_name: String,
    pub kind: TransportKind,
    pub ssh_key_file_name: Option<String>,
    pub https_username: Option<String>,
    pub available: bool,
    pub bound_repository_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransportConfigSnapshot {
    pub values: BTreeMap<String, Vec<String>>,
}

impl TransportConfigSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportDriftStatus {
    Clean,
    Drifted,
}

impl TransportDriftStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Drifted => "drifted",
        }
    }
}

impl FromStr for TransportDriftStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "clean" => Ok(Self::Clean),
            "drifted" => Ok(Self::Drifted),
            _ => Err(AppError::InvalidInput(
                "unknown transport drift status".to_owned(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryTransportBinding {
    pub repository_id: String,
    pub transport_profile_id: String,
    pub before_config: TransportConfigSnapshot,
    pub applied_config: TransportConfigSnapshot,
    pub applied_config_hash: String,
    pub drift_status: TransportDriftStatus,
    pub bound_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RepositoryTransportBinding {
    pub fn summary(&self) -> RepositoryTransportBindingSummary {
        RepositoryTransportBindingSummary {
            repository_id: self.repository_id.clone(),
            transport_profile_id: self.transport_profile_id.clone(),
            drift_status: self.drift_status,
            bound_at: self.bound_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryTransportBindingSummary {
    pub repository_id: String,
    pub transport_profile_id: String,
    pub drift_status: TransportDriftStatus,
    pub bound_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileDeletionImpact {
    pub profile_id: String,
    pub repository_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfigRepair {
    pub id: String,
    pub repository_id: String,
    pub before_config: TransportConfigSnapshot,
    pub attempted_config: TransportConfigSnapshot,
    pub error_code: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloneStage {
    Validating,
    AwaitingAuthentication,
    Transferring,
    CheckingOut,
    ApplyingProfile,
    Registering,
    Refreshing,
    Completed,
    Failed,
    Cancelled,
    Partial,
}

impl CloneStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validating => "validating",
            Self::AwaitingAuthentication => "awaitingAuthentication",
            Self::Transferring => "transferring",
            Self::CheckingOut => "checkingOut",
            Self::ApplyingProfile => "applyingProfile",
            Self::Registering => "registering",
            Self::Refreshing => "refreshing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Partial => "partial",
        }
    }
}

impl FromStr for CloneStage {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "validating" => Ok(Self::Validating),
            "awaitingAuthentication" => Ok(Self::AwaitingAuthentication),
            "transferring" => Ok(Self::Transferring),
            "checkingOut" => Ok(Self::CheckingOut),
            "applyingProfile" => Ok(Self::ApplyingProfile),
            "registering" => Ok(Self::Registering),
            "refreshing" => Ok(Self::Refreshing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "partial" => Ok(Self::Partial),
            _ => Err(AppError::InvalidInput("unknown clone stage".to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CloneProjectTarget {
    Existing { project_id: String },
    New { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneOperation {
    pub operation_id: String,
    pub job_id: String,
    pub source_summary: String,
    pub intent_id: Option<String>,
    pub staging_path: String,
    pub owner_marker_path: String,
    pub final_path: String,
    pub project_target: CloneProjectTarget,
    pub current_stage: CloneStage,
    pub filesystem_complete: bool,
    pub repository_id: Option<String>,
    pub project_id: Option<String>,
    pub profile_applied: bool,
    pub provider_binding_complete: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveTransportSource {
    SystemGit,
    Profile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectiveTransport {
    pub repository_id: String,
    pub source: EffectiveTransportSource,
    pub kind: Option<TransportKind>,
    pub profile: Option<TransportProfileSummary>,
    pub drift_status: Option<TransportDriftStatus>,
}

impl EffectiveTransport {
    pub fn system_git(repository_id: impl Into<String>) -> Self {
        Self {
            repository_id: repository_id.into(),
            source: EffectiveTransportSource::SystemGit,
            kind: None,
            profile: None,
            drift_status: None,
        }
    }

    pub fn profile(
        repository_id: impl Into<String>,
        profile: TransportProfileSummary,
        drift_status: TransportDriftStatus,
    ) -> Self {
        Self {
            repository_id: repository_id.into(),
            source: EffectiveTransportSource::Profile,
            kind: Some(profile.kind),
            profile: Some(profile),
            drift_status: Some(drift_status),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloneIntent {
    pub id: String,
    pub repository: RemoteRepository,
    pub available_transports: Vec<TransportKind>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteTransportKind {
    Ssh,
    Https,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryRemoteSummary {
    pub name: String,
    pub fetch_url: String,
    pub push_url: Option<String>,
    pub kind: RemoteTransportKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpstreamCandidate {
    pub remote_name: String,
    pub branch_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepositoryOperationInProgress {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryNetworkState {
    pub repository_id: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub upstream: Option<UpstreamCandidate>,
    pub remotes: Vec<RepositoryRemoteSummary>,
    pub ahead: i64,
    pub behind: i64,
    pub conflicted_count: i64,
    pub in_progress: Option<RepositoryOperationInProgress>,
}

pub type NetworkStage = CloneStage;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkObjectProgress {
    pub completed: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkByteProgress {
    pub transferred: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkProgress {
    pub operation_id: String,
    pub stage: NetworkStage,
    pub fraction: Option<f64>,
    pub objects: Option<NetworkObjectProgress>,
    pub bytes: Option<NetworkByteProgress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkOperationResult {
    pub operation_id: String,
    pub repository_id: String,
    pub remote_name: Option<String>,
    pub job: Job,
    pub snapshot: RepositorySnapshot,
    pub network_state: RepositoryNetworkState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchInput {
    pub repository_id: String,
    pub context: QueryContext,
    pub remote_name: String,
    pub operation_id: String,
    pub interactive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullInput {
    pub repository_id: String,
    pub context: QueryContext,
    pub operation_id: String,
    pub interactive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    pub remote_name: String,
    pub branch_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushInput {
    pub repository_id: String,
    pub context: QueryContext,
    pub target: Option<PushTarget>,
    pub operation_id: String,
    pub interactive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneSource {
    Intent(String),
    Manual(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneInput {
    pub source: CloneSource,
    pub transport_kind: TransportKind,
    pub profile_id: Option<String>,
    pub destination_parent: PathBuf,
    pub folder_name: String,
    pub project_target: CloneProjectTarget,
    pub operation_id: String,
    pub interactive: bool,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    const CONTRACT_FIXTURE: &str = include_str!(
        "../../../../../../packages/contracts/src/__fixtures__/transport-contracts.json"
    );

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ContractFixture {
        ssh_profile: TransportProfileSummary,
        https_profile: TransportProfileSummary,
        binding: RepositoryTransportBindingSummary,
        intent: CloneIntent,
        network_state: RepositoryNetworkState,
        clone_result: serde_json::Value,
        non_fast_forward_error: serde_json::Value,
    }

    #[test]
    fn transport_profile_summary_redacts_the_key_path_and_rejects_unknown_fields() {
        let profile =
            TransportProfile::new_ssh("Work SSH", r"C:\Users\private\.ssh\id_ed25519", true);
        let summary = profile.summary(true, 2);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(serialized.contains("id_ed25519"));
        assert!(!serialized.contains(r"C:\Users\private"));
        assert!(!format!("{profile:?}").contains(r"C:\Users\private"));

        let mut value = serde_json::to_value(summary).unwrap();
        value["sshKeyPath"] = json!(r"C:\Users\private\.ssh\id_ed25519");
        assert!(serde_json::from_value::<TransportProfileSummary>(value).is_err());
    }

    #[test]
    fn rust_transport_models_parse_the_shared_secret_free_contract_fixture() {
        let fixture: ContractFixture = serde_json::from_str(CONTRACT_FIXTURE).unwrap();
        assert_eq!(fixture.ssh_profile.kind, TransportKind::Ssh);
        assert_eq!(fixture.https_profile.kind, TransportKind::Https);
        assert_eq!(fixture.binding.drift_status, TransportDriftStatus::Clean);
        assert_eq!(fixture.intent.repository.full_name, "skills/private-skill");
        assert_eq!(fixture.network_state.branch.as_deref(), Some("main"));
        assert!(fixture.clone_result.is_object());
        assert!(fixture.non_fast_forward_error.is_object());
    }

    #[test]
    fn tagged_project_targets_and_transport_enums_match_contract_values() {
        let existing = CloneProjectTarget::Existing {
            project_id: "3b84198e-bb1a-4f0d-875f-d82f0c18c630".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(existing).unwrap(),
            json!({
                "kind": "existing",
                "projectId": "3b84198e-bb1a-4f0d-875f-d82f0c18c630"
            })
        );
        assert!(
            serde_json::from_value::<CloneProjectTarget>(json!({
                "kind": "new",
                "name": "Repository",
                "rootPath": "C:/private"
            }))
            .is_err()
        );
        assert_eq!(TransportKind::from_str("ssh").unwrap(), TransportKind::Ssh);
        assert_eq!(
            CloneStage::from_str("checkingOut").unwrap(),
            CloneStage::CheckingOut
        );
        assert_eq!(
            serde_json::to_value(CloneStage::AwaitingAuthentication).unwrap(),
            "awaitingAuthentication"
        );
    }

    #[test]
    fn config_snapshots_are_sorted_and_effective_transport_constructors_are_consistent() {
        let mut snapshot = TransportConfigSnapshot::empty();
        snapshot
            .values
            .insert("ssh.variant".to_owned(), vec!["ssh".to_owned()]);
        snapshot
            .values
            .insert("core.sshCommand".to_owned(), vec!["ssh -i key".to_owned()]);
        assert_eq!(
            snapshot.values.keys().cloned().collect::<Vec<_>>(),
            vec!["core.sshCommand", "ssh.variant"]
        );

        let system = EffectiveTransport::system_git("repository");
        assert_eq!(system.source, EffectiveTransportSource::SystemGit);
        assert!(system.profile.is_none());
        let profile = TransportProfile::new_https("Work HTTPS", "creator").summary(true, 1);
        let managed =
            EffectiveTransport::profile("repository", profile, TransportDriftStatus::Clean);
        assert_eq!(managed.kind, Some(TransportKind::Https));
        assert!(managed.profile.is_some());
    }
}
