use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Error)]
pub enum AppError {
    #[error("database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem operation failed")]
    Io(#[from] std::io::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("permission denied")]
    PermissionDenied,
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("secret store operation failed")]
    SecretStore,
    #[error("serialization failed")]
    Serialization(#[from] serde_json::Error),
    /// A Git invocation failed. The payload is retained for internal diagnostics, but the
    /// user-facing display is deliberately stable so command arguments and credentials cannot
    /// accidentally be echoed to a client.
    #[error("git operation failed")]
    Git(String),
    #[error("git command timed out")]
    Timeout,
    #[error("git output exceeded the configured limit")]
    OutputLimit,
    #[error("path is not valid UTF-8")]
    NonUtf8Path,
    #[error("repository trust is required before this write")]
    TrustRequired,
    #[error("user action required: {0}")]
    UserActionRequired(String),
    #[error("signed commit failed")]
    SigningFailed {
        reason: String,
        repository_id: String,
    },
    #[error("operation completed with partial results: {0}")]
    PartialResult(String),
    #[error("operation canceled")]
    Canceled,
    #[error("{0}")]
    Provider(ProviderFailure),
    #[error("{0}")]
    Transport(TransportFailure),
}

impl fmt::Debug for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Do not include command arguments, repository URLs, or helper stderr in diagnostics.
        // In particular, the payload of `Git` can originate from a credential helper.
        let label = match self {
            Self::Database(_) => "Database",
            Self::Io(_) => "Io",
            Self::InvalidInput(_) => "InvalidInput",
            Self::PermissionDenied => "PermissionDenied",
            Self::NotFound(_) => "NotFound",
            Self::SecretStore => "SecretStore",
            Self::Serialization(_) => "Serialization",
            Self::Git(_) => "Git",
            Self::Timeout => "Timeout",
            Self::OutputLimit => "OutputLimit",
            Self::NonUtf8Path => "NonUtf8Path",
            Self::TrustRequired => "TrustRequired",
            Self::UserActionRequired(_) => "UserActionRequired",
            Self::SigningFailed { .. } => "SigningFailed",
            Self::PartialResult(_) => "PartialResult",
            Self::Canceled => "Canceled",
            Self::Provider(_) => "Provider",
            Self::Transport(_) => "Transport",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCategory {
    Validation,
    UserActionRequired,
    Retryable,
    PartialResult,
    InternalFatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryActionKind {
    Retry,
    OpenSettings,
    Reauthorize,
    ResolveConflict,
    ExportDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAction {
    pub id: String,
    pub label: String,
    pub kind: RecoveryActionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEnvelope {
    pub code: String,
    pub category: ErrorCategory,
    pub message: String,
    pub operation_id: Option<String>,
    pub plugin_id: Option<String>,
    pub resource_id: Option<String>,
    pub failed_step: Option<String>,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub recovery_actions: Vec<RecoveryAction>,
    pub details: Option<serde_json::Map<String, Value>>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TransportFailure(Box<TransportFailureData>);

#[derive(Clone, PartialEq, Eq)]
struct TransportFailureData {
    code: &'static str,
    category: ErrorCategory,
    message: &'static str,
    retryable: bool,
    recovery_actions: Vec<RecoveryAction>,
    operation_id: Option<String>,
    plugin_id: Option<String>,
    resource_id: Option<String>,
    failed_step: Option<String>,
}

impl TransportFailure {
    pub fn authentication_required() -> Self {
        Self::new(
            "git.transport.authentication-required",
            ErrorCategory::UserActionRequired,
            "Git authentication is required",
            false,
            vec![recovery(
                "authenticate-git-transport",
                "Authenticate and retry",
                RecoveryActionKind::Reauthorize,
            )],
        )
    }

    pub fn authentication_cancelled() -> Self {
        Self::new(
            "git.transport.authentication-cancelled",
            ErrorCategory::UserActionRequired,
            "Git authentication was canceled",
            false,
            vec![recovery(
                "retry-git-authentication",
                "Retry authentication",
                RecoveryActionKind::Retry,
            )],
        )
    }

    pub fn permission_denied() -> Self {
        Self::new(
            "git.transport.permission-denied",
            ErrorCategory::UserActionRequired,
            "Remote permission was denied",
            false,
            vec![recovery(
                "review-remote-permissions",
                "Review Remote permissions",
                RecoveryActionKind::OpenSettings,
            )],
        )
    }

    pub fn host_key_unverified() -> Self {
        Self::new(
            "git.transport.host-key-unverified",
            ErrorCategory::UserActionRequired,
            "SSH Host Key is not verified",
            false,
            vec![recovery(
                "verify-ssh-host-key",
                "Review SSH Host Key",
                RecoveryActionKind::OpenSettings,
            )],
        )
    }

    pub fn network_unreachable() -> Self {
        Self::new(
            "git.transport.network-unreachable",
            ErrorCategory::Retryable,
            "Git Remote is unreachable",
            true,
            vec![retry_transport()],
        )
    }

    pub fn tls() -> Self {
        Self::new(
            "git.transport.tls",
            ErrorCategory::UserActionRequired,
            "Git TLS verification failed",
            false,
            vec![recovery(
                "review-git-certificate",
                "Review certificate settings",
                RecoveryActionKind::OpenSettings,
            )],
        )
    }

    pub fn remote_not_found() -> Self {
        Self::new(
            "git.transport.remote-not-found",
            ErrorCategory::UserActionRequired,
            "Git Remote was not found",
            false,
            vec![recovery(
                "review-git-remote",
                "Review Remote",
                RecoveryActionKind::OpenSettings,
            )],
        )
    }

    pub fn upstream_required() -> Self {
        Self::new(
            "git.transport.upstream-required",
            ErrorCategory::UserActionRequired,
            "An upstream branch is required",
            false,
            vec![recovery(
                "select-push-target",
                "Select Push target",
                RecoveryActionKind::ResolveConflict,
            )],
        )
    }

    pub fn detached_head() -> Self {
        Self::new(
            "git.transport.detached-head",
            ErrorCategory::UserActionRequired,
            "HEAD is detached",
            false,
            vec![recovery(
                "select-local-branch",
                "Select a local Branch",
                RecoveryActionKind::ResolveConflict,
            )],
        )
    }

    pub fn operation_in_progress() -> Self {
        Self::new(
            "git.transport.operation-in-progress",
            ErrorCategory::UserActionRequired,
            "Another Git operation requires attention",
            false,
            vec![recovery(
                "resolve-repository-state",
                "Open repository status",
                RecoveryActionKind::ResolveConflict,
            )],
        )
    }

    pub fn non_fast_forward() -> Self {
        Self::new(
            "git.transport.non-fast-forward",
            ErrorCategory::UserActionRequired,
            "Remote history requires integration",
            false,
            vec![recovery(
                "resolve-history",
                "Open repository status",
                RecoveryActionKind::ResolveConflict,
            )],
        )
    }

    pub fn repository_busy() -> Self {
        Self::new(
            "git.transport.repository-busy",
            ErrorCategory::Retryable,
            "Repository is busy",
            true,
            vec![retry_transport()],
        )
    }

    pub fn profile_mismatch() -> Self {
        Self::new(
            "git.transport.profile-mismatch",
            ErrorCategory::UserActionRequired,
            "Transport Profile does not match the Remote",
            false,
            vec![recovery(
                "select-transport-profile",
                "Select Transport Profile",
                RecoveryActionKind::OpenSettings,
            )],
        )
    }

    pub fn config_drift() -> Self {
        Self::new(
            "git.transport.config-drift",
            ErrorCategory::UserActionRequired,
            "Managed Git configuration changed externally",
            false,
            vec![recovery(
                "resolve-transport-drift",
                "Resolve configuration drift",
                RecoveryActionKind::ResolveConflict,
            )],
        )
    }

    pub fn destination_exists() -> Self {
        Self::new(
            "git.transport.destination-exists",
            ErrorCategory::Validation,
            "Clone destination already exists",
            false,
            Vec::new(),
        )
    }

    pub fn unsafe_path() -> Self {
        Self::new(
            "git.transport.unsafe-path",
            ErrorCategory::Validation,
            "Clone path is unsafe",
            false,
            Vec::new(),
        )
    }

    pub fn cancelled() -> Self {
        Self::new(
            "git.transport.cancelled",
            ErrorCategory::Validation,
            "Git network operation was canceled",
            false,
            Vec::new(),
        )
    }

    pub fn timeout() -> Self {
        Self::new(
            "git.transport.timeout",
            ErrorCategory::Retryable,
            "Git network operation timed out",
            true,
            vec![retry_transport()],
        )
    }

    pub fn partial() -> Self {
        Self::new(
            "git.transport.partial",
            ErrorCategory::PartialResult,
            "Git operation completed only partially",
            false,
            vec![recovery(
                "retry-partial-transport-step",
                "Retry incomplete step",
                RecoveryActionKind::Retry,
            )],
        )
    }

    pub fn interrupted() -> Self {
        Self::new(
            "git.transport.interrupted",
            ErrorCategory::UserActionRequired,
            "Git network operation was interrupted",
            false,
            vec![recovery(
                "review-interrupted-transport",
                "Review interrupted operation",
                RecoveryActionKind::ResolveConflict,
            )],
        )
    }

    pub fn code(&self) -> &'static str {
        self.0.code
    }

    pub fn with_operation(mut self, operation_id: impl Into<String>) -> Self {
        self.0.operation_id = Some(operation_id.into());
        self
    }

    pub fn with_plugin(mut self, plugin_id: impl Into<String>) -> Self {
        self.0.plugin_id = Some(plugin_id.into());
        self
    }

    pub fn with_resource(mut self, resource_id: impl Into<String>) -> Self {
        self.0.resource_id = Some(resource_id.into());
        self
    }

    pub fn with_failed_step(mut self, failed_step: impl Into<String>) -> Self {
        self.0.failed_step = Some(failed_step.into());
        self
    }

    fn new(
        code: &'static str,
        category: ErrorCategory,
        message: &'static str,
        retryable: bool,
        recovery_actions: Vec<RecoveryAction>,
    ) -> Self {
        Self(Box::new(TransportFailureData {
            code,
            category,
            message,
            retryable,
            recovery_actions,
            operation_id: None,
            plugin_id: None,
            resource_id: None,
            failed_step: None,
        }))
    }

    pub fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            code: self.0.code.to_owned(),
            category: self.0.category,
            message: self.0.message.to_owned(),
            operation_id: self.0.operation_id.clone(),
            plugin_id: self.0.plugin_id.clone(),
            resource_id: self.0.resource_id.clone(),
            failed_step: self.0.failed_step.clone(),
            retryable: self.0.retryable,
            retry_after_ms: None,
            recovery_actions: self.0.recovery_actions.clone(),
            details: None,
        }
    }
}

impl fmt::Display for TransportFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.message)
    }
}

impl fmt::Debug for TransportFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransportFailure")
            .field("code", &self.0.code)
            .field("operation_id", &self.0.operation_id)
            .field("plugin_id", &self.0.plugin_id)
            .field("resource_id", &self.0.resource_id)
            .field("failed_step", &self.0.failed_step)
            .finish()
    }
}

fn retry_transport() -> RecoveryAction {
    recovery(
        "retry-git-transport",
        "Retry operation",
        RecoveryActionKind::Retry,
    )
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderFailure(Box<ProviderFailureData>);

#[derive(Clone, PartialEq, Eq)]
struct ProviderFailureData {
    code: &'static str,
    category: ErrorCategory,
    message: &'static str,
    retryable: bool,
    retry_after_ms: Option<u64>,
    recovery_actions: Vec<RecoveryAction>,
    operation_id: Option<String>,
    plugin_id: Option<String>,
    resource_id: Option<String>,
    failed_step: Option<String>,
}

impl ProviderFailure {
    pub fn authentication() -> Self {
        Self::new(
            "provider.authentication-required",
            ErrorCategory::UserActionRequired,
            "Provider authentication is required",
            false,
            None,
            vec![recovery(
                "reauthorize-provider",
                "Reconnect account",
                RecoveryActionKind::Reauthorize,
            )],
        )
    }

    pub fn permission() -> Self {
        Self::new(
            "provider.permission-insufficient",
            ErrorCategory::UserActionRequired,
            "Provider permission is insufficient",
            false,
            None,
            vec![recovery(
                "review-provider-permissions",
                "Review Provider permissions",
                RecoveryActionKind::OpenSettings,
            )],
        )
    }

    pub fn rate_limited(retry_after_ms: Option<u64>) -> Self {
        Self::new(
            "provider.rate-limited",
            ErrorCategory::Retryable,
            "Provider rate limit was reached",
            true,
            retry_after_ms,
            vec![recovery(
                "retry-provider-request",
                "Retry request",
                RecoveryActionKind::Retry,
            )],
        )
    }

    pub fn unreachable(retryable: bool) -> Self {
        Self::new(
            "provider.instance-unreachable",
            if retryable {
                ErrorCategory::Retryable
            } else {
                ErrorCategory::UserActionRequired
            },
            "Provider instance is unreachable",
            retryable,
            None,
            vec![recovery(
                "retry-provider-instance",
                "Retry instance",
                RecoveryActionKind::Retry,
            )],
        )
    }

    pub fn tls() -> Self {
        Self::new(
            "provider.tls-failed",
            ErrorCategory::UserActionRequired,
            "Provider TLS verification failed",
            false,
            None,
            vec![recovery(
                "review-provider-certificate",
                "Review certificate settings",
                RecoveryActionKind::OpenSettings,
            )],
        )
    }

    pub fn invalid_cursor() -> Self {
        Self::new(
            "provider.cursor-invalid",
            ErrorCategory::Validation,
            "Provider cursor is invalid or expired",
            false,
            None,
            vec![recovery(
                "reload-provider-repositories",
                "Reload repositories",
                RecoveryActionKind::Retry,
            )],
        )
    }

    pub fn partial() -> Self {
        Self::new(
            "provider.partial-result",
            ErrorCategory::PartialResult,
            "Provider returned a partial result",
            true,
            None,
            vec![recovery(
                "retry-provider-page",
                "Retry page",
                RecoveryActionKind::Retry,
            )],
        )
    }

    pub fn invalid_response() -> Self {
        Self::new(
            "provider.response-invalid",
            ErrorCategory::InternalFatal,
            "Provider returned an invalid response",
            false,
            None,
            vec![recovery(
                "export-provider-diagnostics",
                "Export diagnostics",
                RecoveryActionKind::ExportDiagnostics,
            )],
        )
    }

    pub fn canceled() -> Self {
        Self::new(
            "provider.request-canceled",
            ErrorCategory::Validation,
            "Provider request was canceled",
            false,
            None,
            Vec::new(),
        )
    }

    pub fn busy(retry_after_ms: Option<u64>) -> Self {
        Self::new(
            "provider.request-busy",
            ErrorCategory::Retryable,
            "Provider request capacity is busy",
            true,
            retry_after_ms,
            vec![recovery(
                "retry-provider-request",
                "Retry request",
                RecoveryActionKind::Retry,
            )],
        )
    }

    pub fn code(&self) -> &'static str {
        self.0.code
    }

    pub fn with_request_context(
        mut self,
        plugin_id: impl Into<String>,
        operation_id: impl Into<String>,
    ) -> Self {
        self.0.plugin_id = Some(plugin_id.into());
        self.0.operation_id = Some(operation_id.into());
        self
    }

    pub fn with_resource_id(mut self, resource_id: impl Into<String>) -> Self {
        self.0.resource_id = Some(resource_id.into());
        self
    }

    pub fn with_failed_step(mut self, failed_step: impl Into<String>) -> Self {
        self.0.failed_step = Some(failed_step.into());
        self
    }

    fn new(
        code: &'static str,
        category: ErrorCategory,
        message: &'static str,
        retryable: bool,
        retry_after_ms: Option<u64>,
        recovery_actions: Vec<RecoveryAction>,
    ) -> Self {
        Self(Box::new(ProviderFailureData {
            code,
            category,
            message,
            retryable,
            retry_after_ms,
            recovery_actions,
            operation_id: None,
            plugin_id: None,
            resource_id: None,
            failed_step: None,
        }))
    }

    fn envelope(&self) -> ErrorEnvelope {
        ErrorEnvelope {
            code: self.0.code.to_owned(),
            category: self.0.category,
            message: self.0.message.to_owned(),
            operation_id: self.0.operation_id.clone(),
            plugin_id: self.0.plugin_id.clone(),
            resource_id: self.0.resource_id.clone(),
            failed_step: self.0.failed_step.clone(),
            retryable: self.0.retryable,
            retry_after_ms: self.0.retry_after_ms,
            recovery_actions: self.0.recovery_actions.clone(),
            details: None,
        }
    }
}

impl fmt::Display for ProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.message)
    }
}

impl fmt::Debug for ProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderFailure")
            .field("code", &self.0.code)
            .field("operation_id", &self.0.operation_id)
            .field("plugin_id", &self.0.plugin_id)
            .field("resource_id", &self.0.resource_id)
            .field("failed_step", &self.0.failed_step)
            .finish()
    }
}

fn recovery(id: &str, label: &str, kind: RecoveryActionKind) -> RecoveryAction {
    RecoveryAction {
        id: id.to_owned(),
        label: label.to_owned(),
        kind,
    }
}

impl From<AppError> for ErrorEnvelope {
    fn from(error: AppError) -> Self {
        if let AppError::Transport(failure) = &error {
            return failure.envelope();
        }
        if let AppError::Provider(failure) = &error {
            return failure.envelope();
        }
        let code = match &error {
            AppError::Database(_) => "storage.database",
            AppError::Io(_) => "storage.io",
            AppError::InvalidInput(_) => "validation.invalid",
            AppError::PermissionDenied => "permission.denied",
            AppError::NotFound(_) => "resource.not-found",
            AppError::SecretStore => "secrets.unavailable",
            AppError::Serialization(_) => "serialization.json",
            AppError::Git(_) => "git.failed",
            AppError::Timeout => "git.timeout",
            AppError::OutputLimit => "git.output-limit",
            AppError::NonUtf8Path => "validation.non-utf8-path",
            AppError::TrustRequired => "git.trust-required",
            AppError::UserActionRequired(_) => "user.action-required",
            AppError::SigningFailed { .. } => "git.signing-failed",
            AppError::PartialResult(_) => "git.partial-result",
            AppError::Canceled => "operation.canceled",
            AppError::Provider(_) => unreachable!("Provider failures return above"),
            AppError::Transport(_) => unreachable!("Transport failures return above"),
        };
        let category = match &error {
            AppError::InvalidInput(_) | AppError::NotFound(_) => ErrorCategory::Validation,
            AppError::NonUtf8Path => ErrorCategory::Validation,
            AppError::PermissionDenied
            | AppError::TrustRequired
            | AppError::UserActionRequired(_)
            | AppError::SigningFailed { .. } => ErrorCategory::UserActionRequired,
            AppError::PartialResult(_) => ErrorCategory::PartialResult,
            AppError::Canceled => ErrorCategory::Validation,
            AppError::Timeout => ErrorCategory::Retryable,
            AppError::Database(_)
            | AppError::Io(_)
            | AppError::SecretStore
            | AppError::Serialization(_)
            | AppError::Git(_)
            | AppError::OutputLimit => ErrorCategory::InternalFatal,
            AppError::Provider(_) => unreachable!("Provider failures return above"),
            AppError::Transport(_) => unreachable!("Transport failures return above"),
        };
        let (failed_step, resource_id, details) = match &error {
            AppError::SigningFailed {
                reason,
                repository_id,
            } => {
                let mut details = serde_json::Map::new();
                details.insert(
                    "reason".to_owned(),
                    Value::String(classify_signing_failure(reason).to_owned()),
                );
                (
                    Some("signCommit".to_owned()),
                    Some(repository_id.clone()),
                    Some(details),
                )
            }
            _ => (None, None, None),
        };
        Self {
            code: code.to_owned(),
            category,
            message: error.to_string(),
            operation_id: None,
            plugin_id: None,
            resource_id,
            failed_step,
            retryable: matches!(error, AppError::Timeout),
            retry_after_ms: if matches!(error, AppError::Timeout) {
                Some(250)
            } else {
                None
            },
            recovery_actions: Vec::new(),
            details,
        }
    }
}

fn classify_signing_failure(reason: &str) -> &'static str {
    let reason = reason.to_ascii_lowercase();
    if [
        "permission denied",
        "access denied",
        "access is denied",
        "operation not permitted",
    ]
    .iter()
    .any(|marker| reason.contains(marker))
    {
        return "signing access denied";
    }
    let unavailable = [
        "not found",
        "no such file",
        "unavailable",
        "invalid",
        "could not load",
    ]
    .iter()
    .any(|marker| reason.contains(marker));
    if reason.contains("key") && unavailable {
        return "signing key unavailable";
    }
    if reason.contains("program") && unavailable {
        return "signing program unavailable";
    }
    "signing program failed"
}

#[cfg(test)]
mod tests {
    use super::{AppError, ErrorCategory, ErrorEnvelope, ProviderFailure, TransportFailure};

    #[test]
    fn transport_failure_envelopes_never_echo_remote_or_key_material() {
        let error = AppError::Transport(
            TransportFailure::authentication_required()
                .with_operation("b95c216a-dac4-45d1-8169-8dbfbc0c0315")
                .with_resource("repository/0f85befd-5246-4a3e-9db0-7807e3df97a4")
                .with_failed_step("awaitingAuthentication"),
        );
        let debug = format!("{error:?}");
        let envelope = ErrorEnvelope::from(error);
        let serialized = serde_json::to_string(&envelope).unwrap();
        assert_eq!(envelope.code, "git.transport.authentication-required");
        assert_eq!(
            envelope.operation_id.as_deref(),
            Some("b95c216a-dac4-45d1-8169-8dbfbc0c0315")
        );
        for secret in [
            "ghp_super_secret",
            r"C:\Users\name\.ssh\id_ed25519",
            "user:password@",
        ] {
            assert!(!serialized.contains(secret));
            assert!(!debug.contains(secret));
        }
    }

    #[test]
    fn every_transport_failure_constructor_has_a_unique_stable_code() {
        let failures = [
            TransportFailure::authentication_required(),
            TransportFailure::authentication_cancelled(),
            TransportFailure::permission_denied(),
            TransportFailure::host_key_unverified(),
            TransportFailure::network_unreachable(),
            TransportFailure::tls(),
            TransportFailure::remote_not_found(),
            TransportFailure::upstream_required(),
            TransportFailure::detached_head(),
            TransportFailure::operation_in_progress(),
            TransportFailure::non_fast_forward(),
            TransportFailure::repository_busy(),
            TransportFailure::profile_mismatch(),
            TransportFailure::config_drift(),
            TransportFailure::destination_exists(),
            TransportFailure::unsafe_path(),
            TransportFailure::cancelled(),
            TransportFailure::timeout(),
            TransportFailure::partial(),
            TransportFailure::interrupted(),
        ];
        let codes = failures
            .iter()
            .map(TransportFailure::code)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(codes.len(), failures.len());
        assert!(codes.iter().all(|code| code.starts_with("git.transport.")));
    }

    #[test]
    fn provider_failures_serialize_without_tokens_or_urls() {
        let error = AppError::Provider(
            ProviderFailure::authentication()
                .with_request_context(
                    "git-ramus.provider-center",
                    "f84223af-c753-4209-be36-12d381375fcb",
                )
                .with_resource_id("provider-account/7f3c0214-373c-4d43-b0c7-cdaed1cbcc50"),
        );
        let debug = format!("{error:?}");
        let envelope = ErrorEnvelope::from(error);
        let json = serde_json::to_string(&envelope).unwrap();
        assert_eq!(envelope.code, "provider.authentication-required");
        assert_eq!(
            envelope.operation_id.as_deref(),
            Some("f84223af-c753-4209-be36-12d381375fcb")
        );
        assert!(!json.contains("glpat-"));
        assert!(!json.contains("gitlab.example/private"));
        assert!(!debug.contains("glpat-"));
    }

    #[test]
    fn permission_error_has_a_stable_redacted_envelope() {
        let envelope = ErrorEnvelope::from(AppError::PermissionDenied);
        assert_eq!(envelope.code, "permission.denied");
        assert_eq!(envelope.category, ErrorCategory::UserActionRequired);
        assert!(!envelope.retryable);
        assert!(envelope.retry_after_ms.is_none());
        assert!(envelope.recovery_actions.is_empty());
        assert!(envelope.details.is_none());
    }

    #[test]
    fn signing_and_drift_errors_have_a_stable_user_action_envelope() {
        let envelope = ErrorEnvelope::from(AppError::UserActionRequired(
            "configure a signing tool".to_owned(),
        ));
        assert_eq!(envelope.code, "user.action-required");
        assert_eq!(envelope.category, ErrorCategory::UserActionRequired);
        assert!(!envelope.retryable);
        assert!(envelope.retry_after_ms.is_none());
        assert!(envelope.details.is_none());
    }

    #[test]
    fn signing_error_details_classify_untrusted_stderr_without_exposing_it() {
        let envelope = ErrorEnvelope::from(AppError::SigningFailed {
            reason:
                "signer exploded token=ghp_super_secret path=C:\\Users\\secret\\id key-id=DEADBEEF"
                    .to_owned(),
            repository_id: "repository-id".to_owned(),
        });
        let serialized = serde_json::to_string(&envelope).expect("envelope serializes");

        assert_eq!(envelope.code, "git.signing-failed");
        assert_eq!(envelope.category, ErrorCategory::UserActionRequired);
        assert_eq!(envelope.resource_id.as_deref(), Some("repository-id"));
        assert_eq!(envelope.failed_step.as_deref(), Some("signCommit"));
        assert_eq!(
            envelope
                .details
                .as_ref()
                .and_then(|details| details.get("reason"))
                .and_then(serde_json::Value::as_str),
            Some("signing program failed")
        );
        for secret in [
            "ghp_super_secret",
            "C:\\\\Users",
            "DEADBEEF",
            "signer exploded",
        ] {
            assert!(!serialized.contains(secret), "leaked {secret}");
        }
    }
}
