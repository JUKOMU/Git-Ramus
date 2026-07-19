use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, LINK, USER_AGENT};
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
use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl};

const ACCEPT_VALUE: &str = "application/vnd.github+json";
const API_VERSION: &str = "2026-03-10";
const USER_AGENT_VALUE: &str = "Git-Ramus/0.1";

#[derive(Debug, Clone, Copy, Default)]
pub struct GithubProvider;

impl RepositoryDiscoveryProvider for GithubProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Github
    }

    fn validate_instance<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
    ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
        Box::pin(async move {
            let cancellation = CancellationToken::new();
            let response = client
                .get("/meta", &[], github_headers(None)?, &cancellation)
                .await?;
            ensure_success(&response, false)?;
            Ok(InstanceMetadata {
                server_version: None,
            })
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
                .get("/user", &[], github_headers(Some(secret))?, &cancellation)
                .await?;
            ensure_success(&response, false)?;
            let user: GithubUser = parse_json(&response.body)?;
            if user.id == 0 || user.login.trim().is_empty() {
                return Err(invalid_response());
            }
            let avatar_url = user
                .avatar_url
                .map(validate_optional_https_url)
                .transpose()?;
            Ok(AccountIdentity {
                provider_user_id: user.id.to_string(),
                username: user.login,
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
            let visibility = match request.query.visibility {
                Some(ProviderVisibility::Public) => "public",
                Some(ProviderVisibility::Private) => "private",
                Some(ProviderVisibility::Internal) | None => "all",
            };
            let sort = match request.query.sort {
                ProviderRepositorySort::Name => "full_name",
                ProviderRepositorySort::Updated => "updated",
            };
            let direction = match request.query.direction {
                ProviderRepositoryDirection::Asc => "asc",
                ProviderRepositoryDirection::Desc => "desc",
            };
            let query = [
                (
                    "affiliation",
                    "owner,collaborator,organization_member".to_owned(),
                ),
                ("visibility", visibility.to_owned()),
                ("sort", sort.to_owned()),
                ("direction", direction.to_owned()),
                ("per_page", "100".to_owned()),
                ("page", page.to_string()),
            ];
            let response = context
                .client
                .get(
                    "/user/repos",
                    &query,
                    github_headers(Some(context.secret))?,
                    context.cancellation,
                )
                .await?;
            ensure_success(&response, false)?;
            let repositories: Vec<GithubRepository> = parse_json(&response.body)?;
            let items = repositories
                .into_iter()
                .map(|repository| map_repository(context.client, repository))
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
            let path = repository_api_path(identity)?;
            let response = context
                .client
                .get(
                    &path,
                    &[],
                    github_headers(Some(context.secret))?,
                    context.cancellation,
                )
                .await?;
            ensure_success(&response, true)?;
            map_repository(context.client, parse_json(&response.body)?)
        })
    }

    fn detect_remote(
        &self,
        instance: &NormalizedInstance,
        remote: &NormalizedRemoteUrl,
    ) -> Option<RemoteRepositoryIdentity> {
        let segments = remote.path.split('/').collect::<Vec<_>>();
        if instance.host != "github.com"
            || !instance.root_path.is_empty()
            || remote.host != "github.com"
            || remote.port.is_some()
            || segments.len() != 2
            || segments.iter().any(|segment| segment.is_empty())
        {
            return None;
        }
        Some(RemoteRepositoryIdentity::Path {
            path: remote.path.clone(),
        })
    }
}

fn github_headers(secret: Option<&str>) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(ACCEPT_VALUE));
    headers.insert(
        "x-github-api-version",
        HeaderValue::from_static(API_VERSION),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    if let Some(secret) = secret {
        let mut authorization = Vec::with_capacity("Bearer ".len() + secret.len());
        authorization.extend_from_slice(b"Bearer ");
        authorization.extend_from_slice(secret.as_bytes());
        let value = HeaderValue::from_bytes(&authorization);
        authorization.zeroize();
        let value = value.map_err(|_| AppError::Provider(ProviderFailure::authentication()))?;
        headers.insert(AUTHORIZATION, value);
    }
    Ok(headers)
}

fn ensure_success(response: &BoundedResponse, verification: bool) -> Result<(), AppError> {
    match response.status {
        StatusCode::OK => Ok(()),
        StatusCode::UNAUTHORIZED => Err(AppError::Provider(ProviderFailure::authentication())),
        StatusCode::FORBIDDEN
            if response
                .headers
                .get("x-ratelimit-remaining")
                .and_then(|value| value.to_str().ok())
                == Some("0") =>
        {
            Err(AppError::Provider(ProviderFailure::rate_limited(
                response.retry_after_ms,
            )))
        }
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

fn map_repository(
    client: &ScopedHttpClient,
    repository: GithubRepository,
) -> Result<RemoteRepository, AppError> {
    if repository.id == 0
        || repository.owner.login.trim().is_empty()
        || repository.name.trim().is_empty()
        || repository.full_name != format!("{}/{}", repository.owner.login, repository.name)
    {
        return Err(invalid_response());
    }
    validate_repository_urls(&repository)?;
    let visibility = match repository.visibility.as_deref() {
        Some("public") => ProviderVisibility::Public,
        Some("private") => ProviderVisibility::Private,
        Some("internal") => ProviderVisibility::Internal,
        None if repository.private => ProviderVisibility::Private,
        None => ProviderVisibility::Public,
        Some(_) => return Err(invalid_response()),
    };
    let permission = if repository.permissions.admin {
        ProviderPermission::Admin
    } else if repository.permissions.push || repository.permissions.maintain {
        ProviderPermission::Write
    } else {
        ProviderPermission::Read
    };
    Ok(RemoteRepository {
        provider_kind: ProviderKind::Github,
        instance_id: client.instance_id().to_owned(),
        repository_id: repository.id.to_string(),
        namespace: repository.owner.login,
        name: repository.name,
        full_name: repository.full_name,
        web_url: repository.html_url,
        https_url: repository.clone_url,
        ssh_url: repository.ssh_url,
        default_branch: repository.default_branch,
        visibility,
        archived: repository.archived,
        fork: repository.fork,
        permission,
        updated_at: repository.updated_at,
    })
}

fn validate_repository_urls(repository: &GithubRepository) -> Result<(), AppError> {
    let expected_web_path = format!("/{}", repository.full_name);
    let expected_clone_path = format!("/{0}.git", repository.full_name);
    validate_github_https_url(&repository.html_url, &expected_web_path)?;
    validate_github_https_url(&repository.clone_url, &expected_clone_path)?;
    if repository.ssh_url != format!("git@github.com:{}.git", repository.full_name) {
        return Err(invalid_response());
    }
    Ok(())
}

fn validate_github_https_url(value: &str, expected_path: &str) -> Result<(), AppError> {
    let url = Url::parse(value).map_err(|_| invalid_response())?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != expected_path
    {
        return Err(invalid_response());
    }
    Ok(())
}

fn validate_optional_https_url(value: String) -> Result<String, AppError> {
    let url = Url::parse(&value).map_err(|_| invalid_response())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid_response());
    }
    Ok(value)
}

fn repository_api_path(identity: RemoteRepositoryIdentity) -> Result<String, AppError> {
    match identity {
        RemoteRepositoryIdentity::Id { repository_id } => {
            let id = repository_id
                .parse::<u64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(invalid_response)?;
            Ok(format!("/repositories/{id}"))
        }
        RemoteRepositoryIdentity::Path { path } => {
            let segments = path.split('/').collect::<Vec<_>>();
            if segments.len() != 2 || segments.iter().any(|segment| segment.trim().is_empty()) {
                return Err(invalid_response());
            }
            let mut encoded =
                Url::parse("https://placeholder.invalid/").map_err(|_| invalid_response())?;
            encoded
                .path_segments_mut()
                .map_err(|_| invalid_response())?
                .pop_if_empty()
                .push("repos")
                .push(segments[0])
                .push(segments[1]);
            Ok(encoded.path().to_owned())
        }
    }
}

fn next_page(
    client: &ScopedHttpClient,
    headers: &HeaderMap,
) -> Result<Option<AdapterCursor>, AppError> {
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
    Ok(next_page.map(AdapterCursor::Page))
}

fn rate_limit(response: &BoundedResponse) -> Option<ProviderRateLimitState> {
    let parse = |name: &str| {
        response
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    };
    let limit = parse("x-ratelimit-limit");
    let remaining = parse("x-ratelimit-remaining");
    let reset_at = parse("x-ratelimit-reset").and_then(|seconds| {
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
struct GithubUser {
    id: u64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct GithubRepository {
    id: u64,
    name: String,
    full_name: String,
    owner: GithubOwner,
    html_url: String,
    clone_url: String,
    ssh_url: String,
    default_branch: Option<String>,
    visibility: Option<String>,
    #[serde(default)]
    private: bool,
    archived: bool,
    fork: bool,
    #[serde(default)]
    permissions: GithubPermissions,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct GithubOwner {
    login: String,
}

#[derive(Default, Deserialize)]
struct GithubPermissions {
    #[serde(default)]
    push: bool,
    #[serde(default)]
    admin: bool,
    #[serde(default)]
    maintain: bool,
}

#[cfg(test)]
mod tests {
    use super::{GithubProvider, RepositoryDiscoveryProvider};
    use crate::providers::model::{ProviderKind, RemoteRepositoryIdentity};
    use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl, RemoteTransport};

    #[test]
    fn detects_only_github_repository_remotes() {
        let provider = GithubProvider;
        let instance = NormalizedInstance {
            base_url: "https://github.com".to_owned(),
            api_base_url: "https://api.github.com".to_owned(),
            host: "github.com".to_owned(),
            root_path: String::new(),
        };
        let remote = NormalizedRemoteUrl {
            transport: RemoteTransport::Https,
            host: "github.com".to_owned(),
            port: None,
            path: "octo/private-skill".to_owned(),
        };
        assert_eq!(provider.kind(), ProviderKind::Github);
        assert_eq!(
            provider.detect_remote(&instance, &remote),
            Some(RemoteRepositoryIdentity::Path {
                path: "octo/private-skill".to_owned()
            })
        );
    }
}
