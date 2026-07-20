use url::Url;

use super::model::TransportKind;
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRemoteUrl {
    pub kind: TransportKind,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub sanitized_display: String,
    pub execution_url: String,
}

pub fn validate_clone_url(input: &str) -> Result<ValidatedRemoteUrl, AppError> {
    if input.is_empty() || input.chars().any(char::is_control) || is_remote_helper(input) {
        return Err(invalid_remote());
    }
    if input.contains("://") {
        return validate_url(input);
    }
    validate_scp_url(input)
}

fn validate_url(input: &str) -> Result<ValidatedRemoteUrl, AppError> {
    let parsed = Url::parse(input).map_err(|_| invalid_remote())?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(invalid_remote());
    }
    let host = parsed
        .host_str()
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or_else(invalid_remote)?;
    let path = normalize_repository_path(parsed.path())?;
    let port = canonical_port(parsed.scheme(), parsed.port());
    let authority = authority(&host, port);
    let (kind, execution_url) = match parsed.scheme() {
        "https"
            if parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.port_or_known_default().is_some() =>
        {
            (
                TransportKind::Https,
                format!("https://{authority}/{path}.git"),
            )
        }
        "ssh"
            if parsed.password().is_none()
                && safe_ssh_username(parsed.username())
                && parsed.port_or_known_default().is_some() =>
        {
            (
                TransportKind::Ssh,
                format!("ssh://{}@{authority}/{path}.git", parsed.username()),
            )
        }
        _ => return Err(invalid_remote()),
    };
    Ok(ValidatedRemoteUrl {
        kind,
        host,
        port,
        path,
        sanitized_display: execution_url.clone(),
        execution_url,
    })
}

fn validate_scp_url(input: &str) -> Result<ValidatedRemoteUrl, AppError> {
    let (username, host_and_path) = input.split_once('@').ok_or_else(invalid_remote)?;
    if !safe_ssh_username(username) || host_and_path.contains('@') {
        return Err(invalid_remote());
    }
    let (raw_host, raw_path) = host_and_path.split_once(':').ok_or_else(invalid_remote)?;
    if raw_host.is_empty()
        || raw_host.contains(['/', '\\', ':'])
        || raw_path.starts_with(['/', '\\'])
    {
        return Err(invalid_remote());
    }
    let host = Url::parse(&format!("ssh://{raw_host}/"))
        .map_err(|_| invalid_remote())?
        .host_str()
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or_else(invalid_remote)?;
    let path = normalize_repository_path(raw_path)?;
    let execution_url = format!("{username}@{host}:{path}.git");
    Ok(ValidatedRemoteUrl {
        kind: TransportKind::Ssh,
        host,
        port: None,
        path,
        sanitized_display: execution_url.clone(),
        execution_url,
    })
}

fn normalize_repository_path(value: &str) -> Result<String, AppError> {
    let value = value.trim_matches('/');
    let value = value.strip_suffix(".git").unwrap_or(value);
    let lowered = value.to_ascii_lowercase();
    if value.is_empty()
        || value.contains('\\')
        || value.contains(['?', '#'])
        || lowered.contains("%00")
        || lowered.contains("%2f")
        || lowered.contains("%5c")
        || lowered.contains("%2e")
        || value.split('/').any(|component| {
            component.is_empty()
                || component == "."
                || component == ".."
                || component.chars().any(char::is_control)
        })
    {
        return Err(invalid_remote());
    }
    Ok(value.to_owned())
}

fn safe_ssh_username(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn is_remote_helper(value: &str) -> bool {
    value.split_once("::").is_some_and(|(helper, _)| {
        let mut characters = helper.chars();
        characters
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && characters.all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            })
    })
}

fn canonical_port(scheme: &str, port: Option<u16>) -> Option<u16> {
    match (scheme, port) {
        ("https", Some(443)) | ("ssh", Some(22)) => None,
        (_, value) => value,
    }
}

fn authority(host: &str, port: Option<u16>) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    port.map_or(host.clone(), |value| format!("{host}:{value}"))
}

fn invalid_remote() -> AppError {
    AppError::InvalidInput("Git transport URL is invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use super::validate_clone_url;
    use crate::git::transport::model::TransportKind;

    #[test]
    fn production_clone_urls_allow_only_https_ssh_and_scp_without_embedded_secrets() {
        let https = validate_clone_url("https://GitLab.Example/group/repo.git").unwrap();
        assert_eq!(https.kind, TransportKind::Https);
        assert_eq!(https.execution_url, "https://gitlab.example/group/repo.git");
        let ssh = validate_clone_url("ssh://deploy@GitLab.Example:2222/group/repo.git").unwrap();
        assert_eq!(ssh.kind, TransportKind::Ssh);
        assert_eq!(
            ssh.execution_url,
            "ssh://deploy@gitlab.example:2222/group/repo.git"
        );
        let scp = validate_clone_url("git@GitLab.Example:group/repo.git").unwrap();
        assert_eq!(scp.kind, TransportKind::Ssh);
        assert_eq!(scp.execution_url, "git@gitlab.example:group/repo.git");

        for value in [
            "file:///tmp/repo",
            "../repo",
            "C:/repo",
            "git://gitlab.example/group/repo.git",
            "ext::sh -c evil",
            "https://user:secret@gitlab.example/group/repo.git",
            "https://gitlab.example/group/repo.git?token=secret",
            "ssh://git:secret@gitlab.example/group/repo.git",
            "git@gitlab.example:../repo.git",
            "git@gitlab.example:group/repo.git?token=secret",
            "git@gitlab.example:group/repo.git#fragment",
            "https://gitlab.example/group/repo.git\n--upload-pack=evil",
        ] {
            assert!(validate_clone_url(value).is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn validated_remote_debug_and_display_values_never_retain_rejected_input() {
        let error = validate_clone_url(
            "https://creator:ghp_super_secret@gitlab.example/private/repo.git?token=secret",
        )
        .unwrap_err();
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains("ghp_super_secret"));
        assert!(!rendered.contains("private/repo"));
    }
}
