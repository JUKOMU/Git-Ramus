use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use reqwest::header::{ACCEPT, HeaderMap, HeaderName, HeaderValue, LINK, USER_AGENT};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use crate::error::{AppError, ProviderFailure};
use crate::providers::adapter::{AdapterAccountContext, RepositoryDiscoveryProvider};
use crate::providers::http::{BoundedResponse, ScopedHttpClient};
use crate::providers::model::{
    AccountIdentity, AdapterCursor, AdapterListRequest, AdapterPage, InstanceMetadata,
    ProviderKind, ProviderPermission, ProviderRateLimitState, ProviderRepositoryDirection,
    ProviderRepositorySort, ProviderVisibility, RemoteRepository, RemoteRepositoryIdentity,
};
use crate::providers::url::{
    NormalizedInstance, NormalizedRemoteUrl, RemoteTransport, normalize_remote_url,
};

const PRIVATE_TOKEN: HeaderName = HeaderName::from_static("private-token");
const USER_AGENT_VALUE: &str = "Git-Ramus/0.1";

#[derive(Debug, Clone, Copy, Default)]
pub struct GitlabProvider;

impl RepositoryDiscoveryProvider for GitlabProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gitlab
    }

    fn validate_instance<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
    ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
        Box::pin(async move {
            let cancellation = CancellationToken::new();
            let response = client
                .get("/version", &[], gitlab_headers(None)?, &cancellation)
                .await?;
            match response.status {
                StatusCode::OK => {
                    let metadata: GitlabVersion = parse_json(&response.body)?;
                    if metadata.version.is_empty()
                        || metadata.version.len() > 128
                        || metadata.version.chars().any(char::is_control)
                    {
                        return Err(invalid_response());
                    }
                    Ok(InstanceMetadata {
                        server_version: Some(metadata.version),
                    })
                }
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Ok(InstanceMetadata {
                    server_version: None,
                }),
                status if status.is_server_error() => {
                    Err(AppError::Provider(ProviderFailure::unreachable(true)))
                }
                _ => Err(invalid_response()),
            }
        })
    }

    fn authenticate_account<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
        secret: &'a str,
    ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
        Box::pin(async move {
            let cancellation = CancellationToken::new();
            let response = client
                .get("/user", &[], gitlab_headers(Some(secret))?, &cancellation)
                .await?;
            ensure_success(&response, false)?;
            let user: GitlabUser = parse_json(&response.body)?;
            if user.id == 0 || user.username.trim().is_empty() {
                return Err(invalid_response());
            }
            let avatar_url = user
                .avatar_url
                .map(|value| validate_avatar_url(client, value))
                .transpose()?;
            Ok(AccountIdentity {
                provider_user_id: user.id.to_string(),
                username: user.username,
                display_name: user.name.filter(|name| !name.trim().is_empty()),
                avatar_url,
            })
        })
    }

    fn list_repositories<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        request: AdapterListRequest,
    ) -> BoxFuture<'a, Result<AdapterPage, AppError>> {
        Box::pin(async move {
            let page = match request.cursor {
                None => 1,
                Some(AdapterCursor::Page(page)) if page > 0 => page,
                Some(_) => {
                    return Err(AppError::Provider(ProviderFailure::invalid_cursor()));
                }
            };
            let order_by = match request.query.sort {
                ProviderRepositorySort::Name => "name",
                ProviderRepositorySort::Updated => "last_activity_at",
            };
            let sort = match request.query.direction {
                ProviderRepositoryDirection::Asc => "asc",
                ProviderRepositoryDirection::Desc => "desc",
            };
            let mut query = vec![
                ("membership", "true".to_owned()),
                ("simple", "true".to_owned()),
                ("per_page", "100".to_owned()),
                ("page", page.to_string()),
                ("order_by", order_by.to_owned()),
                ("sort", sort.to_owned()),
            ];
            if !request.query.search.trim().is_empty() {
                query.push(("search", request.query.search.clone()));
            }
            let response = context
                .client
                .get(
                    "/projects",
                    &query,
                    gitlab_headers(Some(context.secret))?,
                    context.cancellation,
                )
                .await?;
            ensure_success(&response, false)?;
            let projects: Vec<GitlabProject> = parse_json(&response.body)?;
            let items = projects
                .into_iter()
                .map(|project| map_project(context.client, project))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(AdapterPage {
                items,
                next_cursor: next_page(context.client, &response.headers)?,
                rate_limit: rate_limit(&response),
            })
        })
    }

    fn get_repository<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        identity: RemoteRepositoryIdentity,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
        Box::pin(async move {
            let path = project_api_path(identity)?;
            let response = context
                .client
                .get(
                    &path,
                    &[],
                    gitlab_headers(Some(context.secret))?,
                    context.cancellation,
                )
                .await?;
            ensure_success(&response, true)?;
            map_project(context.client, parse_json(&response.body)?)
        })
    }

    fn detect_remote(
        &self,
        instance: &NormalizedInstance,
        remote: &NormalizedRemoteUrl,
    ) -> Option<RemoteRepositoryIdentity> {
        if remote.host != instance.host {
            return None;
        }
        let path = match remote.transport {
            RemoteTransport::Https => {
                let instance_url = Url::parse(&instance.base_url).ok()?;
                let expected_port = match instance_url.port() {
                    Some(443) | None => None,
                    port => port,
                };
                if remote.port != expected_port {
                    return None;
                }
                if instance.root_path.is_empty() {
                    remote.path.as_str()
                } else {
                    remote
                        .path
                        .strip_prefix(&format!("{}/", instance.root_path))?
                }
            }
            RemoteTransport::Ssh => remote.path.as_str(),
        };
        if path.split('/').count() < 2
            || path.split('/').any(|segment| segment.is_empty())
            || path.chars().any(char::is_control)
        {
            return None;
        }
        Some(RemoteRepositoryIdentity::Path {
            path: path.to_owned(),
        })
    }
}

fn gitlab_headers(secret: Option<&str>) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    if let Some(secret) = secret {
        let mut token = secret.as_bytes().to_vec();
        let value = HeaderValue::from_bytes(&token);
        token.zeroize();
        headers.insert(
            PRIVATE_TOKEN,
            value.map_err(|_| AppError::Provider(ProviderFailure::authentication()))?,
        );
    }
    Ok(headers)
}

fn ensure_success(response: &BoundedResponse, verification: bool) -> Result<(), AppError> {
    match response.status {
        StatusCode::OK => Ok(()),
        StatusCode::UNAUTHORIZED => Err(AppError::Provider(ProviderFailure::authentication())),
        StatusCode::FORBIDDEN => Err(AppError::Provider(ProviderFailure::permission())),
        StatusCode::NOT_FOUND if verification => {
            Err(AppError::Provider(ProviderFailure::permission()))
        }
        StatusCode::TOO_MANY_REQUESTS => Err(AppError::Provider(ProviderFailure::rate_limited(
            response.retry_after_ms,
        ))),
        status if status.is_server_error() => {
            Err(AppError::Provider(ProviderFailure::unreachable(true)))
        }
        _ => Err(invalid_response()),
    }
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, AppError> {
    serde_json::from_slice(body).map_err(|_| invalid_response())
}

fn map_project(
    client: &ScopedHttpClient,
    project: GitlabProject,
) -> Result<RemoteRepository, AppError> {
    let (namespace, path) = project
        .path_with_namespace
        .rsplit_once('/')
        .filter(|(namespace, path)| !namespace.is_empty() && !path.is_empty())
        .ok_or_else(invalid_response)?;
    if project.id == 0
        || project.name.trim().is_empty()
        || project.name.chars().any(char::is_control)
        || project.path != path
        || project
            .path_with_namespace
            .split('/')
            .any(|segment| segment.is_empty())
    {
        return Err(invalid_response());
    }
    validate_project_urls(client, &project)?;
    let visibility = match project.visibility.as_str() {
        "public" => ProviderVisibility::Public,
        "internal" => ProviderVisibility::Internal,
        "private" => ProviderVisibility::Private,
        _ => return Err(invalid_response()),
    };
    let project_access = project
        .permissions
        .as_ref()
        .and_then(|permissions| permissions.project_access.as_ref())
        .map_or(0, |access| access.access_level);
    let group_access = project
        .permissions
        .as_ref()
        .and_then(|permissions| permissions.group_access.as_ref())
        .map_or(0, |access| access.access_level);
    let permission = match project_access.max(group_access) {
        40.. => ProviderPermission::Admin,
        30..=39 => ProviderPermission::Write,
        _ => ProviderPermission::Read,
    };
    Ok(RemoteRepository {
        provider_kind: ProviderKind::Gitlab,
        instance_id: client.instance_id().to_owned(),
        repository_id: project.id.to_string(),
        namespace: namespace.to_owned(),
        name: project.name,
        full_name: project.path_with_namespace,
        web_url: project.web_url,
        https_url: project.http_url_to_repo,
        ssh_url: project.ssh_url_to_repo,
        default_branch: project.default_branch,
        visibility,
        archived: project.archived,
        fork: project.forked_from_project.is_some(),
        permission,
        updated_at: project.last_activity_at,
    })
}

fn validate_project_urls(
    client: &ScopedHttpClient,
    project: &GitlabProject,
) -> Result<(), AppError> {
    let root = gitlab_root_url(client)?;
    let expected_web = append_path(&root, &project.path_with_namespace, false)?;
    let expected_clone = append_path(&root, &project.path_with_namespace, true)?;
    let web = Url::parse(&project.web_url).map_err(|_| invalid_response())?;
    let clone = Url::parse(&project.http_url_to_repo).map_err(|_| invalid_response())?;
    if web != expected_web || clone != expected_clone {
        return Err(invalid_response());
    }
    let ssh = normalize_remote_url(&project.ssh_url_to_repo).map_err(|_| invalid_response())?;
    if ssh.host != client.api_origin().host_str().unwrap_or_default()
        || ssh.path != project.path_with_namespace
    {
        return Err(invalid_response());
    }
    Ok(())
}

fn gitlab_root_url(client: &ScopedHttpClient) -> Result<Url, AppError> {
    let mut root = client.api_origin().clone();
    let api_path = root.path().trim_end_matches('/');
    let root_path = api_path
        .strip_suffix("/api/v4")
        .ok_or_else(invalid_response)?;
    let path = if root_path.is_empty() {
        "/".to_owned()
    } else {
        format!("{root_path}/")
    };
    root.set_path(&path);
    Ok(root)
}

fn append_path(root: &Url, full_name: &str, git_suffix: bool) -> Result<Url, AppError> {
    let mut result = root.clone();
    let mut segments = result.path_segments_mut().map_err(|_| invalid_response())?;
    segments.pop_if_empty();
    let mut names = full_name.split('/').peekable();
    while let Some(segment) = names.next() {
        if segment.is_empty() {
            return Err(invalid_response());
        }
        if git_suffix && names.peek().is_none() {
            segments.push(&format!("{segment}.git"));
        } else {
            segments.push(segment);
        }
    }
    drop(segments);
    Ok(result)
}

fn validate_avatar_url(client: &ScopedHttpClient, value: String) -> Result<String, AppError> {
    let url = Url::parse(&value).map_err(|_| invalid_response())?;
    let allowed_scheme = url.scheme() == "https"
        || (client.api_origin().scheme() == "http" && url.scheme() == "http");
    if !allowed_scheme
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid_response());
    }
    Ok(value)
}

fn project_api_path(identity: RemoteRepositoryIdentity) -> Result<String, AppError> {
    let identity = match identity {
        RemoteRepositoryIdentity::Id { repository_id } => {
            let id = repository_id
                .parse::<u64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(invalid_response)?;
            id.to_string()
        }
        RemoteRepositoryIdentity::Path { path } => {
            if path.split('/').count() < 2
                || path.split('/').any(|segment| segment.is_empty())
                || path.chars().any(char::is_control)
            {
                return Err(invalid_response());
            }
            path
        }
    };
    let mut encoded = Url::parse("https://placeholder.invalid/").map_err(|_| invalid_response())?;
    encoded
        .path_segments_mut()
        .map_err(|_| invalid_response())?
        .pop_if_empty()
        .push("projects")
        .push(&identity);
    Ok(encoded.path().to_owned())
}

fn next_page(
    client: &ScopedHttpClient,
    headers: &HeaderMap,
) -> Result<Option<AdapterCursor>, AppError> {
    let link_page = link_next_page(client, headers)?;
    let header_page = match headers.get("x-next-page") {
        None => None,
        Some(value) => {
            let value = value.to_str().map_err(|_| invalid_response())?.trim();
            if value.is_empty() {
                None
            } else {
                Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|page| *page > 0)
                        .ok_or_else(invalid_response)?,
                )
            }
        }
    };
    if link_page.is_some() && header_page.is_some() && link_page != header_page {
        return Err(invalid_response());
    }
    Ok(link_page.or(header_page).map(AdapterCursor::Page))
}

fn link_next_page(client: &ScopedHttpClient, headers: &HeaderMap) -> Result<Option<u64>, AppError> {
    let mut next_page = None;
    for value in headers.get_all(LINK) {
        let value = value.to_str().map_err(|_| invalid_response())?;
        for link in value.split(',') {
            let mut parts = link.trim().split(';');
            let Some(target) = parts.next() else {
                continue;
            };
            let is_next = parts.any(|parameter| {
                parameter
                    .trim()
                    .strip_prefix("rel=")
                    .map(|rel| {
                        rel.trim_matches('"')
                            .split_ascii_whitespace()
                            .any(|rel| rel == "next")
                    })
                    .unwrap_or(false)
            });
            if !is_next {
                continue;
            }
            let target = target
                .strip_prefix('<')
                .and_then(|target| target.strip_suffix('>'))
                .ok_or_else(invalid_response)?;
            let url = Url::parse(target).map_err(|_| invalid_response())?;
            if !client.is_same_origin(&url) {
                return Err(invalid_response());
            }
            let pages = url
                .query_pairs()
                .filter(|(name, _)| name == "page")
                .map(|(_, value)| value.parse::<u64>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| invalid_response())?;
            if pages.len() != 1 || pages[0] == 0 || next_page.replace(pages[0]).is_some() {
                return Err(invalid_response());
            }
        }
    }
    Ok(next_page)
}

fn rate_limit(response: &BoundedResponse) -> Option<ProviderRateLimitState> {
    let parse = |name: &str| {
        response
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    };
    let limit = parse("ratelimit-limit");
    let remaining = parse("ratelimit-remaining");
    let reset_at = parse("ratelimit-reset").and_then(|seconds| {
        i64::try_from(seconds)
            .ok()
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
    });
    if limit.is_none()
        && remaining.is_none()
        && reset_at.is_none()
        && response.retry_after_ms.is_none()
    {
        return None;
    }
    Some(ProviderRateLimitState {
        limit,
        remaining,
        reset_at,
        retry_after_ms: response.retry_after_ms,
    })
}

fn invalid_response() -> AppError {
    AppError::Provider(ProviderFailure::invalid_response())
}

#[derive(Deserialize)]
struct GitlabVersion {
    version: String,
}

#[derive(Deserialize)]
struct GitlabUser {
    id: u64,
    username: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct GitlabProject {
    id: u64,
    name: String,
    path: String,
    path_with_namespace: String,
    default_branch: Option<String>,
    visibility: String,
    ssh_url_to_repo: String,
    http_url_to_repo: String,
    web_url: String,
    archived: bool,
    forked_from_project: Option<serde_json::Value>,
    permissions: Option<GitlabPermissions>,
    last_activity_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct GitlabPermissions {
    project_access: Option<GitlabAccess>,
    group_access: Option<GitlabAccess>,
}

#[derive(Deserialize)]
struct GitlabAccess {
    access_level: u64,
}

#[cfg(test)]
mod tests {
    use super::{GitlabProvider, RepositoryDiscoveryProvider};
    use crate::providers::model::{ProviderKind, RemoteRepositoryIdentity};
    use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl, RemoteTransport};

    #[test]
    fn detects_nested_gitlab_remotes_below_a_relative_root() {
        let provider = GitlabProvider;
        let instance = NormalizedInstance {
            base_url: "https://gitlab.example/gitlab".to_owned(),
            api_base_url: "https://gitlab.example/gitlab/api/v4".to_owned(),
            host: "gitlab.example".to_owned(),
            root_path: "gitlab".to_owned(),
        };
        let remote = NormalizedRemoteUrl {
            transport: RemoteTransport::Https,
            host: "gitlab.example".to_owned(),
            port: None,
            path: "gitlab/group/subgroup/skill-set".to_owned(),
        };
        assert_eq!(provider.kind(), ProviderKind::Gitlab);
        assert_eq!(
            provider.detect_remote(&instance, &remote),
            Some(RemoteRepositoryIdentity::Path {
                path: "group/subgroup/skill-set".to_owned()
            })
        );

        let ssh = NormalizedRemoteUrl {
            transport: RemoteTransport::Ssh,
            host: "gitlab.example".to_owned(),
            port: None,
            path: "gitlab/group/skill-set".to_owned(),
        };
        assert_eq!(
            provider.detect_remote(&instance, &ssh),
            Some(RemoteRepositoryIdentity::Path {
                path: "gitlab/group/skill-set".to_owned()
            })
        );
    }
}
