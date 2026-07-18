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
        };
        let category = match &error {
            AppError::InvalidInput(_) | AppError::NotFound(_) => ErrorCategory::Validation,
            AppError::NonUtf8Path => ErrorCategory::Validation,
            AppError::PermissionDenied => ErrorCategory::UserActionRequired,
            AppError::Timeout => ErrorCategory::Retryable,
            AppError::Database(_)
            | AppError::Io(_)
            | AppError::SecretStore
            | AppError::Serialization(_)
            | AppError::Git(_)
            | AppError::OutputLimit => ErrorCategory::InternalFatal,
        };
        Self {
            code: code.to_owned(),
            category,
            message: error.to_string(),
            operation_id: None,
            plugin_id: None,
            resource_id: None,
            failed_step: None,
            retryable: matches!(error, AppError::Timeout),
            retry_after_ms: if matches!(error, AppError::Timeout) {
                Some(250)
            } else {
                None
            },
            recovery_actions: Vec::new(),
            details: None,
        }
    }
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
}
