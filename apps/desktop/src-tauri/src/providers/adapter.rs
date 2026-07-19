use futures_util::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;
use crate::providers::http::ScopedHttpClient;
use crate::providers::model::{
    AccountIdentity, AdapterListRequest, AdapterPage, InstanceMetadata, ProviderKind,
    RemoteRepository, RemoteRepositoryIdentity,
};
use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl};

pub struct AdapterAccountContext<'a> {
    pub client: &'a ScopedHttpClient,
    pub secret: &'a str,
    pub cancellation: &'a CancellationToken,
}

pub trait RepositoryDiscoveryProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    fn validate_instance<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
    ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>>;

    fn authenticate_account<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
        secret: &'a str,
    ) -> BoxFuture<'a, Result<AccountIdentity, AppError>>;

    fn list_repositories<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        request: AdapterListRequest,
    ) -> BoxFuture<'a, Result<AdapterPage, AppError>>;

    fn get_repository<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        identity: RemoteRepositoryIdentity,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>>;

    fn detect_remote(
        &self,
        instance: &NormalizedInstance,
        remote: &NormalizedRemoteUrl,
    ) -> Option<RemoteRepositoryIdentity>;
}
