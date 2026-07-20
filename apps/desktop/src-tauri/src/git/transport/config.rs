use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::path::Path;

use super::model::{TransportKind, TransportProfile};
use super::url::ValidatedRemoteUrl;
use crate::error::AppError;

#[derive(Clone, PartialEq, Eq)]
pub struct ManagedConfigPlan {
    pub kind: TransportKind,
    pub values: BTreeMap<String, String>,
}

impl Debug for ManagedConfigPlan {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedConfigPlan")
            .field("kind", &self.kind)
            .field("managed_keys", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

pub fn config_plan(
    profile: &TransportProfile,
    remote: &ValidatedRemoteUrl,
) -> Result<ManagedConfigPlan, AppError> {
    if profile.kind != remote.kind {
        return Err(invalid_profile());
    }
    match profile.kind {
        TransportKind::Ssh => ssh_config_plan(profile),
        TransportKind::Https => https_config_plan(profile, remote),
    }
}

fn ssh_config_plan(profile: &TransportProfile) -> Result<ManagedConfigPlan, AppError> {
    let key_path = profile
        .ssh_key_path
        .as_deref()
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .ok_or_else(invalid_profile)?;
    let path = Path::new(key_path);
    if !path.is_absolute() || !path.is_file() || profile.ssh_variant.as_deref() != Some("ssh") {
        return Err(invalid_profile());
    }
    let identities_only = profile.ssh_identities_only.ok_or_else(invalid_profile)?;
    if profile.https_username.is_some() || profile.https_use_http_path.is_some() {
        return Err(invalid_profile());
    }
    let mut command = format!("ssh -i {}", shell_quote(key_path)?);
    if identities_only {
        command.push_str(" -o IdentitiesOnly=yes");
    }
    Ok(ManagedConfigPlan {
        kind: TransportKind::Ssh,
        values: BTreeMap::from([
            ("core.sshCommand".to_owned(), command),
            ("ssh.variant".to_owned(), "ssh".to_owned()),
        ]),
    })
}

fn https_config_plan(
    profile: &TransportProfile,
    remote: &ValidatedRemoteUrl,
) -> Result<ManagedConfigPlan, AppError> {
    let username = profile
        .https_username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
        .ok_or_else(invalid_profile)?;
    if profile.https_use_http_path != Some(true)
        || profile.ssh_key_path.is_some()
        || profile.ssh_variant.is_some()
        || profile.ssh_identities_only.is_some()
    {
        return Err(invalid_profile());
    }
    Ok(ManagedConfigPlan {
        kind: TransportKind::Https,
        values: BTreeMap::from([
            (
                format!("credential.{}.username", remote.execution_url),
                username.to_owned(),
            ),
            ("credential.useHttpPath".to_owned(), "true".to_owned()),
        ]),
    })
}

fn shell_quote(value: &str) -> Result<String, AppError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(invalid_profile());
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

fn invalid_profile() -> AppError {
    AppError::InvalidInput("Git transport profile is invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::config_plan;
    use crate::git::transport::model::{TransportConfigSnapshot, TransportKind, TransportProfile};
    use crate::git::transport::url::validate_clone_url;

    #[test]
    fn ssh_plan_is_built_only_from_typed_fields_and_quotes_the_selected_key() {
        let directory = tempfile::tempdir().unwrap();
        let key_path = directory.path().join("work key'ed25519");
        std::fs::write(&key_path, "test fixture key").unwrap();
        let profile = TransportProfile::new_ssh("Key", key_path.to_string_lossy(), true);
        let remote = validate_clone_url("git@gitlab.example:group/repo.git").unwrap();
        let plan = config_plan(&profile, &remote).unwrap();
        assert_eq!(plan.kind, TransportKind::Ssh);
        assert_eq!(
            plan.values.get("ssh.variant").map(String::as_str),
            Some("ssh")
        );
        let command = plan.values.get("core.sshCommand").unwrap();
        assert!(command.starts_with("ssh -i "));
        assert!(command.contains("IdentitiesOnly=yes"));
        assert!(!command.contains('\n'));
        assert!(!command.contains('\0'));
        assert!(!command.contains("test fixture key"));
        assert!(command.contains("'\\''"));
        let debug = format!("{plan:?}");
        let private_directory_name = directory.path().file_name().unwrap().to_string_lossy();
        assert!(!debug.contains(private_directory_name.as_ref()), "{debug}");
    }

    #[test]
    fn https_plan_scopes_non_secret_username_to_the_normalized_remote() {
        let profile = TransportProfile::new_https("Web", "creator");
        let remote = validate_clone_url("https://GitLab.Example/group/repo.git").unwrap();
        let plan = config_plan(&profile, &remote).unwrap();
        assert_eq!(plan.kind, TransportKind::Https);
        assert_eq!(
            plan.values
                .get("credential.useHttpPath")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            plan.values
                .get("credential.https://gitlab.example/group/repo.git.username")
                .map(String::as_str),
            Some("creator")
        );
        assert!(
            config_plan(
                &profile,
                &validate_clone_url("git@gitlab.example:group/repo.git").unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn config_snapshot_hash_is_stable_unambiguous_and_order_independent() {
        let first = TransportConfigSnapshot {
            values: BTreeMap::from([
                ("ssh.variant".to_owned(), vec!["ssh".to_owned()]),
                ("core.sshCommand".to_owned(), vec!["ssh -i key".to_owned()]),
            ]),
        };
        let second = TransportConfigSnapshot {
            values: BTreeMap::from([
                ("core.sshCommand".to_owned(), vec!["ssh -i key".to_owned()]),
                ("ssh.variant".to_owned(), vec!["ssh".to_owned()]),
            ]),
        };
        let different_boundaries = TransportConfigSnapshot {
            values: BTreeMap::from([("ssh.variantssh".to_owned(), vec![String::new()])]),
        };
        assert_eq!(first.sha256(), second.sha256());
        assert_ne!(first.sha256(), different_boundaries.sha256());
        assert_eq!(first.sha256().len(), 64);
    }
}
