//! Deterministic Provider adapter compiled only for debug E2E builds.

use futures_util::future::BoxFuture;

use crate::error::{AppError, ProviderFailure};
use crate::providers::adapter::{AdapterAccountContext, RepositoryDiscoveryProvider};
use crate::providers::http::ScopedHttpClient;
use crate::providers::model::{
    AccountIdentity, AdapterListRequest, AdapterPage, InstanceMetadata, ProviderKind,
    ProviderPermission, ProviderVisibility, RemoteRepository, RemoteRepositoryIdentity,
};
use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl};

pub const E2E_PROVIDER_TOKEN: &str = "e2e-provider-token";
pub const E2E_PROVIDER_HOST: &str = "gitlab.example.test";
pub const E2E_PROVIDER_BASE_URL: &str = "https://gitlab.example.test";
pub const E2E_PROVIDER_REPOSITORY_ID: &str = "4242";
pub const E2E_PROVIDER_REPOSITORY_PATH: &str = "skills/private-skill";
pub const E2E_PROVIDER_SSH_URL: &str = "git@gitlab.example.test:skills/private-skill.git";

#[derive(Debug, Clone, Copy, Default)]
pub struct E2eProvider;

impl RepositoryDiscoveryProvider for E2eProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gitlab
    }

    fn validate_instance<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
    ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
        Box::pin(async move {
            ensure_fixture_instance(client)?;
            Ok(InstanceMetadata {
                server_version: Some("18.0.0-e2e".to_owned()),
            })
        })
    }

    fn authenticate_account<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
        secret: &'a str,
    ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
        Box::pin(async move {
            ensure_fixture_instance(client)?;
            ensure_fixture_secret(secret)?;
            Ok(AccountIdentity {
                provider_user_id: "9001".to_owned(),
                username: "e2e-provider".to_owned(),
                display_name: Some("E2E Provider".to_owned()),
                avatar_url: None,
            })
        })
    }

    fn list_repositories<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        request: AdapterListRequest,
    ) -> BoxFuture<'a, Result<AdapterPage, AppError>> {
        Box::pin(async move {
            ensure_fixture_instance(context.client)?;
            ensure_fixture_secret(context.secret)?;
            if context.cancellation.is_cancelled() {
                return Err(AppError::Provider(ProviderFailure::canceled()));
            }
            if request.cursor.is_some() {
                return Err(AppError::Provider(ProviderFailure::invalid_cursor()));
            }
            Ok(AdapterPage {
                items: vec![fixture_repository(context.client.instance_id())],
                next_cursor: None,
                rate_limit: None,
            })
        })
    }

    fn get_repository<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        identity: RemoteRepositoryIdentity,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
        Box::pin(async move {
            ensure_fixture_instance(context.client)?;
            ensure_fixture_secret(context.secret)?;
            if context.cancellation.is_cancelled() {
                return Err(AppError::Provider(ProviderFailure::canceled()));
            }
            let matches = match identity {
                RemoteRepositoryIdentity::Id { repository_id } => {
                    repository_id == E2E_PROVIDER_REPOSITORY_ID
                }
                RemoteRepositoryIdentity::Path { path } => path == E2E_PROVIDER_REPOSITORY_PATH,
            };
            if !matches {
                return Err(AppError::NotFound("Provider repository".to_owned()));
            }
            Ok(fixture_repository(context.client.instance_id()))
        })
    }

    fn detect_remote(
        &self,
        instance: &NormalizedInstance,
        remote: &NormalizedRemoteUrl,
    ) -> Option<RemoteRepositoryIdentity> {
        (instance.base_url == E2E_PROVIDER_BASE_URL
            && instance.host == E2E_PROVIDER_HOST
            && instance.root_path.is_empty()
            && remote.host == E2E_PROVIDER_HOST
            && remote.path == E2E_PROVIDER_REPOSITORY_PATH)
            .then(|| RemoteRepositoryIdentity::Path {
                path: E2E_PROVIDER_REPOSITORY_PATH.to_owned(),
            })
    }
}

fn ensure_fixture_instance(client: &ScopedHttpClient) -> Result<(), AppError> {
    let origin = client.api_origin();
    if origin.scheme() == "https"
        && origin.host_str() == Some(E2E_PROVIDER_HOST)
        && origin.port_or_known_default() == Some(443)
        && origin.path() == "/api/v4/"
    {
        Ok(())
    } else {
        Err(AppError::Provider(ProviderFailure::invalid_response()))
    }
}

fn ensure_fixture_secret(secret: &str) -> Result<(), AppError> {
    if secret == E2E_PROVIDER_TOKEN {
        Ok(())
    } else {
        Err(AppError::Provider(ProviderFailure::authentication()))
    }
}

fn fixture_repository(instance_id: &str) -> RemoteRepository {
    RemoteRepository {
        provider_kind: ProviderKind::Gitlab,
        instance_id: instance_id.to_owned(),
        repository_id: E2E_PROVIDER_REPOSITORY_ID.to_owned(),
        namespace: "skills".to_owned(),
        name: "private-skill".to_owned(),
        full_name: E2E_PROVIDER_REPOSITORY_PATH.to_owned(),
        web_url: format!("{E2E_PROVIDER_BASE_URL}/{E2E_PROVIDER_REPOSITORY_PATH}"),
        https_url: format!("{E2E_PROVIDER_BASE_URL}/{E2E_PROVIDER_REPOSITORY_PATH}.git"),
        ssh_url: E2E_PROVIDER_SSH_URL.to_owned(),
        default_branch: Some("main".to_owned()),
        visibility: ProviderVisibility::Private,
        archived: false,
        fork: false,
        permission: ProviderPermission::Write,
        updated_at: chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .expect("fixed E2E timestamp is valid")
            .with_timezone(&chrono::Utc),
    }
}

#[cfg(test)]
mod tests {
    use super::{E2E_PROVIDER_BASE_URL, E2E_PROVIDER_TOKEN, E2eProvider};
    use crate::providers::adapter::RepositoryDiscoveryProvider;
    use crate::providers::http::ScopedHttpClient;

    #[tokio::test]
    async fn fixture_adapter_accepts_only_the_fixed_instance_and_token() {
        let adapter = E2eProvider;
        let client = ScopedHttpClient::for_test_http(&format!("{E2E_PROVIDER_BASE_URL}/api/v4"))
            .expect("fixture client builds");
        assert!(adapter.validate_instance(&client).await.is_ok());
        assert!(
            adapter
                .authenticate_account(&client, E2E_PROVIDER_TOKEN)
                .await
                .is_ok()
        );
        assert!(
            adapter
                .authenticate_account(&client, "not-the-fixture-token")
                .await
                .is_err()
        );
    }
}
