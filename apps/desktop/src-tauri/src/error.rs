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

impl From<AppError> for ErrorEnvelope {
    fn from(error: AppError) -> Self {
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
        };
        let category = match &error {
            AppError::InvalidInput(_) | AppError::NotFound(_) => ErrorCategory::Validation,
            AppError::NonUtf8Path => ErrorCategory::Validation,
            AppError::PermissionDenied
            | AppError::TrustRequired
            | AppError::UserActionRequired(_)
            | AppError::SigningFailed { .. } => ErrorCategory::UserActionRequired,
            AppError::PartialResult(_) => ErrorCategory::PartialResult,
            AppError::Timeout => ErrorCategory::Retryable,
            AppError::Database(_)
            | AppError::Io(_)
            | AppError::SecretStore
            | AppError::Serialization(_)
            | AppError::Git(_)
            | AppError::OutputLimit => ErrorCategory::InternalFatal,
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
    use super::{AppError, ErrorCategory, ErrorEnvelope};

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
