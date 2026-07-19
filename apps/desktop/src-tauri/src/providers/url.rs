use url::Url;

use crate::error::AppError;
use crate::providers::model::{ProviderKind, RemoteRepositoryIdentity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRemoteUrl {
    pub transport: RemoteTransport,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteTransport {
    Https,
    Ssh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedInstance {
    pub base_url: String,
    pub api_base_url: String,
    pub host: String,
    pub root_path: String,
}

pub fn normalize_instance_base(
    input: &str,
    kind: ProviderKind,
) -> Result<NormalizedInstance, AppError> {
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err(invalid_instance());
    }
    let parsed = Url::parse(input).map_err(|_| invalid_instance())?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid_instance());
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or_else(invalid_instance)?;
    let port = canonical_port(parsed.scheme(), parsed.port());
    let root_path = normalize_instance_path(parsed.path())?;
    if kind == ProviderKind::Github
        && (host != "github.com" || port.is_some() || !root_path.is_empty())
    {
        return Err(invalid_instance());
    }
    if kind == ProviderKind::Github {
        return Ok(NormalizedInstance {
            base_url: "https://github.com".to_owned(),
            api_base_url: "https://api.github.com".to_owned(),
            host,
            root_path,
        });
    }
    let authority = authority(&host, port);
    let base_url = if root_path.is_empty() {
        format!("https://{authority}")
    } else {
        format!("https://{authority}/{root_path}")
    };
    Ok(NormalizedInstance {
        api_base_url: format!("{base_url}/api/v4"),
        base_url,
        host,
        root_path,
    })
}

pub fn normalize_remote_url(input: &str) -> Result<NormalizedRemoteUrl, AppError> {
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err(invalid_remote());
    }
    if let Some(scp) = input.strip_prefix("git@") {
        return normalize_scp_remote(scp);
    }
    let parsed = Url::parse(input).map_err(|_| invalid_remote())?;
    if !matches!(parsed.scheme(), "https" | "ssh") {
        return Err(invalid_remote());
    }
    if parsed.scheme() == "ssh"
        && ((!parsed.username().is_empty() && parsed.username() != "git")
            || parsed.password().is_some())
    {
        return Err(invalid_remote());
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or_else(invalid_remote)?;
    let path = normalize_repository_path(parsed.path())?;
    Ok(NormalizedRemoteUrl {
        transport: if parsed.scheme() == "https" {
            RemoteTransport::Https
        } else {
            RemoteTransport::Ssh
        },
        host,
        port: canonical_port(parsed.scheme(), parsed.port()),
        path,
    })
}

pub fn detect_remote(
    instance: &NormalizedInstance,
    remote: &NormalizedRemoteUrl,
) -> Option<RemoteRepositoryIdentity> {
    if instance.host != remote.host {
        return None;
    }
    let instance_url = Url::parse(&instance.base_url).ok()?;
    if canonical_port(instance_url.scheme(), instance_url.port()) != remote.port {
        return None;
    }
    let path = if instance.root_path.is_empty() {
        remote.path.as_str()
    } else {
        remote
            .path
            .strip_prefix(&format!("{}/", instance.root_path))?
    };
    if path.split('/').count() < 2 {
        return None;
    }
    Some(RemoteRepositoryIdentity::Path {
        path: path.to_owned(),
    })
}

pub fn sanitized_remote_url(remote: &NormalizedRemoteUrl) -> String {
    let host = if remote.host.contains(':') {
        format!("[{}]", remote.host)
    } else {
        remote.host.clone()
    };
    match remote.transport {
        RemoteTransport::Https => {
            let authority = remote
                .port
                .map_or(host.clone(), |port| format!("{host}:{port}"));
            format!("https://{authority}/{}.git", remote.path)
        }
        RemoteTransport::Ssh if remote.port.is_none() && !remote.host.contains(':') => {
            format!("git@{}:{}.git", remote.host, remote.path)
        }
        RemoteTransport::Ssh => {
            let authority = remote
                .port
                .map_or(host.clone(), |port| format!("{host}:{port}"));
            format!("ssh://git@{authority}/{}.git", remote.path)
        }
    }
}

fn normalize_scp_remote(value: &str) -> Result<NormalizedRemoteUrl, AppError> {
    let (host, path) = value.split_once(':').ok_or_else(invalid_remote)?;
    if host.is_empty()
        || host.contains(['/', '\\', '@'])
        || path.contains('\\')
        || path.starts_with('/')
    {
        return Err(invalid_remote());
    }
    let parsed_host = Url::parse(&format!("ssh://{host}/"))
        .map_err(|_| invalid_remote())?
        .host_str()
        .filter(|host| !host.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or_else(invalid_remote)?;
    Ok(NormalizedRemoteUrl {
        transport: RemoteTransport::Ssh,
        host: parsed_host,
        port: None,
        path: normalize_repository_path(path)?,
    })
}

fn normalize_instance_path(path: &str) -> Result<String, AppError> {
    let path = path.trim_matches('/');
    if path.is_empty() {
        return Ok(String::new());
    }
    validate_path(path).map_err(|_| invalid_instance())?;
    Ok(path.to_owned())
}

fn normalize_repository_path(path: &str) -> Result<String, AppError> {
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() {
        return Err(invalid_remote());
    }
    validate_path(path).map_err(|_| invalid_remote())?;
    Ok(path.to_owned())
}

fn validate_path(path: &str) -> Result<(), ()> {
    if path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(());
    }
    Ok(())
}

fn canonical_port(scheme: &str, port: Option<u16>) -> Option<u16> {
    match (scheme, port) {
        ("https", Some(443)) | ("ssh", Some(22)) => None,
        (_, port) => port,
    }
}

fn authority(host: &str, port: Option<u16>) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    port.map_or(host.clone(), |port| format!("{host}:{port}"))
}

fn invalid_instance() -> AppError {
    AppError::InvalidInput("Provider instance URL is invalid".to_owned())
}

fn invalid_remote() -> AppError {
    AppError::InvalidInput("Git remote URL is invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use crate::error::ErrorEnvelope;
    use crate::providers::model::{ProviderKind, RemoteRepositoryIdentity};

    use super::{
        NormalizedInstance, NormalizedRemoteUrl, RemoteTransport, detect_remote,
        normalize_instance_base, normalize_remote_url, sanitized_remote_url,
    };

    #[test]
    fn normalizes_https_ssh_and_scp_remotes_without_retaining_credentials() {
        assert_eq!(
            normalize_remote_url("git@GitLab.Example:group/repo.git").unwrap(),
            NormalizedRemoteUrl {
                transport: RemoteTransport::Ssh,
                host: "gitlab.example".to_owned(),
                port: None,
                path: "group/repo".to_owned(),
            }
        );
        assert_eq!(
            normalize_remote_url("ssh://git@gitlab.example:22/group/Repo.git/").unwrap(),
            NormalizedRemoteUrl {
                transport: RemoteTransport::Ssh,
                host: "gitlab.example".to_owned(),
                port: None,
                path: "group/Repo".to_owned(),
            }
        );
        let sanitized =
            normalize_remote_url("https://token@gitlab.example/group/repo.git?x=secret#fragment")
                .unwrap();
        let debug = format!("{sanitized:?}");
        assert!(!debug.contains("token"));
        assert!(!debug.contains("x=secret"));
        assert_eq!(sanitized.path, "group/repo");

        let error = normalize_remote_url("https://token@/private?x=secret").unwrap_err();
        let envelope = serde_json::to_string(&ErrorEnvelope::from(error)).unwrap();
        assert!(!envelope.contains("token"));
        assert!(!envelope.contains("x=secret"));
        assert!(!envelope.contains("private"));
    }

    #[test]
    fn normalizes_strict_provider_instances_and_relative_gitlab_roots() {
        assert_eq!(
            normalize_instance_base("https://GitHub.com/", ProviderKind::Github).unwrap(),
            NormalizedInstance {
                base_url: "https://github.com".to_owned(),
                api_base_url: "https://api.github.com".to_owned(),
                host: "github.com".to_owned(),
                root_path: String::new(),
            }
        );
        let gitlab =
            normalize_instance_base("https://GitLab.Example:443/root/", ProviderKind::Gitlab)
                .unwrap();
        assert_eq!(gitlab.base_url, "https://gitlab.example/root");
        assert_eq!(gitlab.api_base_url, "https://gitlab.example/root/api/v4");
        assert_eq!(gitlab.root_path, "root");
        assert!(normalize_instance_base("http://gitlab.example", ProviderKind::Gitlab).is_err());
        assert!(
            normalize_instance_base("https://user:pass@gitlab.example", ProviderKind::Gitlab)
                .is_err()
        );
        assert!(
            normalize_instance_base("https://gitlab.example?private=1", ProviderKind::Gitlab)
                .is_err()
        );
    }

    #[test]
    fn detects_only_host_port_and_relative_root_scoped_repository_paths() {
        let instance =
            normalize_instance_base("https://gitlab.example/root", ProviderKind::Gitlab).unwrap();
        let remote =
            normalize_remote_url("https://gitlab.example/root/group/subgroup/repository.git")
                .unwrap();
        assert_eq!(
            detect_remote(&instance, &remote),
            Some(RemoteRepositoryIdentity::Path {
                path: "group/subgroup/repository".to_owned()
            })
        );
        let outside = normalize_remote_url("git@gitlab.example:group/repository.git").unwrap();
        assert_eq!(detect_remote(&instance, &outside), None);
        let other_port =
            normalize_remote_url("ssh://git@gitlab.example:2222/root/group/repository.git")
                .unwrap();
        assert_eq!(detect_remote(&instance, &other_port), None);
    }

    #[test]
    fn sanitizes_remote_urls_without_credentials_or_query_data() {
        let remote = normalize_remote_url(
            "https://token@gitlab.example:8443/group/repo.git?secret=yes#fragment",
        )
        .unwrap();
        assert_eq!(
            sanitized_remote_url(&remote),
            "https://gitlab.example:8443/group/repo.git"
        );
        assert_eq!(
            sanitized_remote_url(
                &normalize_remote_url("git@gitlab.example:group/repo.git").unwrap()
            ),
            "git@gitlab.example:group/repo.git"
        );
    }
}
