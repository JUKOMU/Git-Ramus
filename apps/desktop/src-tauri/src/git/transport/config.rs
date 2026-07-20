use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::{Debug, Formatter};
use std::path::Path;
use std::time::Duration;

use super::model::{TransportConfigSnapshot, TransportKind, TransportProfile};
use super::url::{ValidatedRemoteUrl, validate_clone_url};
use crate::error::AppError;
use crate::git::{GitCommand, GitRunner};

const CONFIG_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MANAGED_CONFIG_ENTRIES: usize = 256;
const MAX_MANAGED_CONFIG_KEY_BYTES: usize = 8 * 1024;
const MAX_MANAGED_CONFIG_VALUE_BYTES: usize = 64 * 1024;
const HOST_OWNED_CONFIG_REGEX: &str = "^(core\\.sshcommand|ssh\\.variant|credential\\.usehttppath|credential\\.https://.*\\.username)$";

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

pub(crate) fn plan_snapshot(plan: &ManagedConfigPlan) -> Result<TransportConfigSnapshot, AppError> {
    let mut values = BTreeMap::new();
    for (key, value) in &plan.values {
        let key = canonical_managed_key(key)?;
        if values.insert(key, vec![value.clone()]).is_some() {
            return Err(invalid_profile());
        }
    }
    Ok(TransportConfigSnapshot { values })
}

pub(crate) fn read_managed_snapshot(
    runner: &dyn GitRunner,
    repository: &Path,
) -> Result<TransportConfigSnapshot, AppError> {
    let output = runner.run(GitCommand {
        repo: repository.to_path_buf(),
        args: vec![
            OsString::from("config"),
            OsString::from("--local"),
            OsString::from("--null"),
            OsString::from("--get-regexp"),
            OsString::from(HOST_OWNED_CONFIG_REGEX),
        ],
        stdin: None,
        timeout: CONFIG_TIMEOUT,
    })?;
    if !output.status.success() {
        if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
            return Ok(TransportConfigSnapshot::empty());
        }
        return Err(config_git_failure());
    }
    parse_managed_config(&output.stdout)
}

pub(crate) fn write_managed_snapshot(
    runner: &dyn GitRunner,
    repository: &Path,
    target: &TransportConfigSnapshot,
) -> Result<(), AppError> {
    ensure_restorable_snapshot(target)?;
    let mut expected = read_managed_snapshot(runner, repository)?;
    let mut keys = expected.values.keys().cloned().collect::<BTreeSet<_>>();
    keys.extend(target.values.keys().cloned());
    if keys.len() > MAX_MANAGED_CONFIG_ENTRIES {
        return Err(AppError::OutputLimit);
    }

    for key in &keys {
        let target_value = target.values.get(key).and_then(|values| values.first());
        if expected.values.get(key).and_then(|values| values.first()) == target_value
            && expected.values.get(key).map(Vec::len).unwrap_or_default()
                == usize::from(target_value.is_some())
        {
            continue;
        }
        let args = if let Some(value) = target_value {
            vec![
                OsString::from("config"),
                OsString::from("--local"),
                OsString::from("--replace-all"),
                OsString::from(key),
                OsString::from(value),
            ]
        } else {
            if !expected.values.contains_key(key) {
                continue;
            }
            vec![
                OsString::from("config"),
                OsString::from("--local"),
                OsString::from("--unset-all"),
                OsString::from(key),
            ]
        };
        let output = runner.run(GitCommand {
            repo: repository.to_path_buf(),
            args,
            stdin: None,
            timeout: CONFIG_TIMEOUT,
        })?;
        if !output.status.success() {
            return Err(config_git_failure());
        }
        if let Some(value) = target_value {
            expected.values.insert(key.clone(), vec![value.clone()]);
        } else {
            expected.values.remove(key);
        }
        if read_managed_snapshot(runner, repository)? != expected {
            return Err(config_git_failure());
        }
    }

    if expected != *target {
        return Err(config_git_failure());
    }
    Ok(())
}

pub(crate) fn ensure_restorable_snapshot(
    snapshot: &TransportConfigSnapshot,
) -> Result<(), AppError> {
    if snapshot.values.len() > MAX_MANAGED_CONFIG_ENTRIES {
        return Err(AppError::OutputLimit);
    }
    for (key, values) in &snapshot.values {
        canonical_managed_key(key)?;
        if values.len() != 1 || values[0].len() > MAX_MANAGED_CONFIG_VALUE_BYTES {
            return Err(AppError::UserActionRequired(
                "managed Git configuration contains unsupported multiple or oversized values"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

fn parse_managed_config(input: &[u8]) -> Result<TransportConfigSnapshot, AppError> {
    if input.is_empty() {
        return Ok(TransportConfigSnapshot::empty());
    }
    if !input.ends_with(&[0]) {
        return Err(AppError::InvalidInput(
            "Git config stream is not NUL terminated".to_owned(),
        ));
    }
    let mut values = BTreeMap::<String, Vec<String>>::new();
    let mut entries = 0_usize;
    for record in input[..input.len() - 1].split(|byte| *byte == 0) {
        let separator = record
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| AppError::InvalidInput("malformed Git config record".to_owned()))?;
        let key = std::str::from_utf8(&record[..separator])
            .map_err(|_| AppError::InvalidInput("Git config key is not UTF-8".to_owned()))?
            .to_owned();
        let value = std::str::from_utf8(&record[separator + 1..])
            .map_err(|_| AppError::InvalidInput("Git config value is not UTF-8".to_owned()))?
            .to_owned();
        if canonical_managed_key(&key)? != key {
            return Err(AppError::InvalidInput(
                "Git returned an unexpected managed config key".to_owned(),
            ));
        }
        if value.len() > MAX_MANAGED_CONFIG_VALUE_BYTES {
            return Err(AppError::OutputLimit);
        }
        entries += 1;
        if entries > MAX_MANAGED_CONFIG_ENTRIES {
            return Err(AppError::OutputLimit);
        }
        values.entry(key).or_default().push(value);
    }
    for values in values.values_mut() {
        values.sort();
    }
    Ok(TransportConfigSnapshot { values })
}

fn canonical_managed_key(key: &str) -> Result<String, AppError> {
    if key.len() > MAX_MANAGED_CONFIG_KEY_BYTES {
        return Err(AppError::OutputLimit);
    }
    match key {
        "core.sshCommand" | "core.sshcommand" => Ok("core.sshcommand".to_owned()),
        "ssh.variant" => Ok("ssh.variant".to_owned()),
        "credential.useHttpPath" | "credential.usehttppath" => {
            Ok("credential.usehttppath".to_owned())
        }
        _ => {
            let url = key
                .strip_prefix("credential.")
                .and_then(|value| value.strip_suffix(".username"))
                .ok_or_else(invalid_managed_key)?;
            let remote = validate_clone_url(url).map_err(|_| invalid_managed_key())?;
            if remote.kind != TransportKind::Https || remote.execution_url != url {
                return Err(invalid_managed_key());
            }
            Ok(format!("credential.{url}.username"))
        }
    }
}

fn invalid_managed_key() -> AppError {
    AppError::InvalidInput("invalid managed Git config key".to_owned())
}

fn config_git_failure() -> AppError {
    AppError::Git("transport config command failed".to_owned())
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

    use super::{config_plan, ensure_restorable_snapshot, parse_managed_config, plan_snapshot};
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

    #[test]
    fn managed_config_parser_preserves_sorted_values_and_rejects_unexpected_keys() {
        let snapshot =
            parse_managed_config(b"credential.usehttppath\ntrue\0credential.usehttppath\nfalse\0")
                .unwrap();
        assert_eq!(
            snapshot.values.get("credential.usehttppath"),
            Some(&vec!["false".to_owned(), "true".to_owned()])
        );
        assert!(
            parse_managed_config(b"core.editor\nevil\0").is_err(),
            "a Git response cannot expand the host-owned key set"
        );
        assert!(ensure_restorable_snapshot(&snapshot).is_err());
    }

    #[test]
    fn config_plans_are_canonicalized_to_the_keys_reported_by_git() {
        let profile = TransportProfile::new_https("Web", "creator");
        let remote = validate_clone_url("https://gitlab.example/group/repo.git").unwrap();
        let plan = config_plan(&profile, &remote).unwrap();
        let snapshot = plan_snapshot(&plan).unwrap();
        assert_eq!(
            snapshot
                .values
                .get("credential.usehttppath")
                .and_then(|values| values.first())
                .map(String::as_str),
            Some("true")
        );
        assert!(
            snapshot
                .values
                .contains_key("credential.https://gitlab.example/group/repo.git.username")
        );
        assert!(!snapshot.values.contains_key("credential.useHttpPath"));
    }
}
