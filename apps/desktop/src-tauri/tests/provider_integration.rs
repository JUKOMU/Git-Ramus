use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use futures_util::future::BoxFuture;
use git_ramus_desktop_lib::db::Database;
use git_ramus_desktop_lib::error::{AppError, ErrorEnvelope};
use git_ramus_desktop_lib::git::model::{Remote, Repository, RepositoryKind};
use git_ramus_desktop_lib::git::repository::RepositoryRepository;
use git_ramus_desktop_lib::providers::adapter::{
    AdapterAccountContext, ProviderAdapterRegistry, RepositoryDiscoveryProvider,
};
use git_ramus_desktop_lib::providers::github::GithubProvider;
use git_ramus_desktop_lib::providers::gitlab::GitlabProvider;
use git_ramus_desktop_lib::providers::http::ScopedHttpClient;
use git_ramus_desktop_lib::providers::model::{
    AccountIdentity, AdapterCursor, AdapterListRequest, AdapterPage, InstanceMetadata,
    ProviderArchivedFilter, ProviderBindingSuggestionStatus, ProviderInstance, ProviderKind,
    ProviderPermission, ProviderRepositoryDirection, ProviderRepositoryQuery,
    ProviderRepositorySort, ProviderVisibility, RemoteRepository, RemoteRepositoryIdentity,
};
use git_ramus_desktop_lib::providers::service::{
    BindRemoteInput, CreateInstanceInput, ProviderService,
};
use git_ramus_desktop_lib::providers::store::ProviderStore;
use git_ramus_desktop_lib::providers::url::{NormalizedInstance, NormalizedRemoteUrl};
use git_ramus_desktop_lib::secrets::{MemorySecretStore, SecretStore, SensitiveString};
use httpmock::{Method::GET, MockServer};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use tempfile::NamedTempFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_util::sync::CancellationToken;

struct ScriptedResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
}

impl ScriptedResponse {
    fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

struct ScriptedServer {
    origin: String,
    hits: Arc<AtomicUsize>,
    requests: Arc<std::sync::Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl ScriptedServer {
    async fn start(responses: Vec<ScriptedResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("scripted server binds");
        let address = listener.local_addr().expect("server has an address");
        let hits = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let task_hits = Arc::clone(&hits);
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            for response in responses {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                task_hits.fetch_add(1, Ordering::SeqCst);
                let request = read_request(&mut socket).await;
                task_requests
                    .lock()
                    .expect("request log lock")
                    .push(request);
                tokio::time::sleep(response.delay).await;
                let reason = match response.status {
                    200 => "OK",
                    302 => "Found",
                    429 => "Too Many Requests",
                    502 => "Bad Gateway",
                    503 => "Service Unavailable",
                    504 => "Gateway Timeout",
                    _ => "Response",
                };
                let mut head = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    response.status,
                    reason,
                    response.body.len()
                );
                for (name, value) in response.headers {
                    head.push_str(&name);
                    head.push_str(": ");
                    head.push_str(&value);
                    head.push_str("\r\n");
                }
                head.push_str("\r\n");
                if socket.write_all(head.as_bytes()).await.is_ok() {
                    let _ = socket.write_all(&response.body).await;
                }
            }
        });
        Self {
            origin: format!("http://{address}"),
            hits,
            requests,
            task,
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("request log lock").clone()
    }
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(stream: &mut (impl AsyncReadExt + Unpin)) -> String {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    while bytes.len() < 64 * 1024 {
        let Ok(read) = stream.read(&mut chunk).await else {
            break;
        };
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn instance(api_base_url: impl Into<String>, custom_ca_path: Option<String>) -> ProviderInstance {
    let now = Utc::now();
    ProviderInstance {
        id: "instance-test".to_owned(),
        provider_kind: ProviderKind::Gitlab,
        display_name: "Test GitLab".to_owned(),
        base_url: "https://gitlab.example".to_owned(),
        api_base_url: api_base_url.into(),
        custom_ca_path,
        last_validated_at: None,
        server_version: None,
        created_at: now,
        updated_at: now,
    }
}

fn error_code(error: AppError) -> String {
    ErrorEnvelope::from(error).code
}

fn authorization_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer never-forward-this"),
    );
    headers
}

#[tokio::test]
async fn scoped_http_follows_only_same_origin_redirects() {
    let server = ScriptedServer::start(vec![
        ScriptedResponse::new(302, Vec::new()).header("Location", "/api/v4/next"),
        ScriptedResponse::new(200, br#"{"ok":true}"#.to_vec()),
    ])
    .await;
    let client = ScopedHttpClient::for_test_http(&format!("{}/api/v4", server.origin))
        .expect("test client builds");

    let response = client
        .get(
            "/redirect",
            &[],
            authorization_headers(),
            &CancellationToken::new(),
        )
        .await
        .expect("same-origin redirect succeeds");

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, br#"{"ok":true}"#);
    assert_eq!(server.hits(), 2);
    assert!(
        server
            .requests()
            .iter()
            .all(|request| request.contains("authorization: Bearer never-forward-this"))
    );
}

#[tokio::test]
async fn scoped_http_rejects_cross_origin_redirect_before_forwarding_authorization() {
    let target = ScriptedServer::start(vec![ScriptedResponse::new(200, "target")]).await;
    let source = ScriptedServer::start(vec![
        ScriptedResponse::new(302, Vec::new())
            .header("Location", format!("{}/capture", target.origin)),
    ])
    .await;
    let client = ScopedHttpClient::for_test_http(&source.origin).expect("test client builds");

    let result = client
        .get(
            "/redirect",
            &[],
            authorization_headers(),
            &CancellationToken::new(),
        )
        .await;

    assert!(result.is_err());
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(source.hits(), 1);
    assert_eq!(target.hits(), 0);
}

#[tokio::test]
async fn scoped_http_stops_streaming_when_the_body_limit_is_exceeded() {
    let server = ScriptedServer::start(vec![ScriptedResponse::new(200, vec![b'x'; 256])]).await;
    let client = ScopedHttpClient::for_test_http_with_limits(
        &server.origin,
        Duration::from_secs(1),
        32,
        [Duration::ZERO, Duration::ZERO],
    )
    .expect("test client builds");

    let error = client
        .get("/large", &[], HeaderMap::new(), &CancellationToken::new())
        .await
        .expect_err("oversized body is rejected");

    assert_eq!(error_code(error), "provider.response-invalid");
    assert_eq!(server.hits(), 1);
}

#[tokio::test]
async fn scoped_http_respects_total_timeout_and_cancellation() {
    let timeout_server = ScriptedServer::start(vec![
        ScriptedResponse::new(200, "late").delayed(Duration::from_millis(150)),
    ])
    .await;
    let timeout_client = ScopedHttpClient::for_test_http_with_limits(
        &timeout_server.origin,
        Duration::from_millis(25),
        1024,
        [Duration::ZERO, Duration::ZERO],
    )
    .expect("timeout client builds");
    let timeout_error = timeout_client
        .get("/slow", &[], HeaderMap::new(), &CancellationToken::new())
        .await
        .expect_err("total timeout is enforced");
    assert_eq!(error_code(timeout_error), "provider.instance-unreachable");

    let cancel_server = ScriptedServer::start(vec![
        ScriptedResponse::new(200, "late").delayed(Duration::from_secs(1)),
    ])
    .await;
    let cancel_client =
        ScopedHttpClient::for_test_http(&cancel_server.origin).expect("cancellation client builds");
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(25)).await;
        trigger.cancel();
    });
    let canceled = cancel_client
        .get("/slow", &[], HeaderMap::new(), &cancellation)
        .await
        .expect_err("cancellation interrupts the request");
    assert_eq!(error_code(canceled), "provider.request-canceled");
}

#[test]
fn scoped_http_rejects_a_missing_custom_ca_before_building_the_client() {
    let missing = std::env::temp_dir().join("git-ramus-ca-that-does-not-exist.pem");
    let error = ScopedHttpClient::build(&instance(
        "https://127.0.0.1:1/api/v4",
        Some(missing.to_string_lossy().into_owned()),
    ))
    .expect_err("missing CA path is rejected");
    assert_eq!(error_code(error), "provider.tls-failed");
}

#[tokio::test]
async fn scoped_http_retries_transient_server_failures_only_within_the_bound() {
    let recovering = ScriptedServer::start(vec![
        ScriptedResponse::new(503, "retry"),
        ScriptedResponse::new(200, "ok"),
    ])
    .await;
    let recovering_client = ScopedHttpClient::for_test_http_with_limits(
        &recovering.origin,
        Duration::from_secs(1),
        1024,
        [Duration::ZERO, Duration::ZERO],
    )
    .expect("test client builds");
    let response = recovering_client
        .get(
            "/repositories",
            &[],
            HeaderMap::new(),
            &CancellationToken::new(),
        )
        .await
        .expect("second response succeeds");
    assert_eq!(response.body, b"ok");
    assert_eq!(recovering.hits(), 2);

    let unavailable = ScriptedServer::start(vec![
        ScriptedResponse::new(503, "retry"),
        ScriptedResponse::new(503, "retry"),
        ScriptedResponse::new(503, "retry"),
    ])
    .await;
    let unavailable_client = ScopedHttpClient::for_test_http_with_limits(
        &unavailable.origin,
        Duration::from_secs(1),
        1024,
        [Duration::ZERO, Duration::ZERO],
    )
    .expect("test client builds");
    let error = unavailable_client
        .get(
            "/repositories",
            &[],
            HeaderMap::new(),
            &CancellationToken::new(),
        )
        .await
        .expect_err("persistent 503 is bounded");
    assert_eq!(error_code(error), "provider.instance-unreachable");
    assert_eq!(unavailable.hits(), 3);
}

#[tokio::test]
async fn scoped_http_returns_retry_after_without_retrying_429() {
    let server = ScriptedServer::start(vec![
        ScriptedResponse::new(429, "slow down").header("Retry-After", "2"),
    ])
    .await;
    let client = ScopedHttpClient::for_test_http(&server.origin).expect("test client builds");

    let response = client
        .get(
            "/repositories",
            &[],
            HeaderMap::new(),
            &CancellationToken::new(),
        )
        .await
        .expect("429 metadata is returned to the adapter");

    assert_eq!(response.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.retry_after_ms, Some(2_000));
    assert_eq!(server.hits(), 1);
}

async fn start_tls_server(cert: CertificateDer<'static>, key: Vec<u8>) -> String {
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        )
        .expect("test TLS config is valid");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("TLS server binds");
    let address = listener.local_addr().expect("TLS server has an address");
    tokio::spawn(async move {
        let Ok((socket, _)) = listener.accept().await else {
            return;
        };
        let Ok(mut tls) = acceptor.accept(socket).await else {
            return;
        };
        let _ = read_request(&mut tls).await;
        let body = br#"{"version":"18.2.0"}"#;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        if tls.write_all(head.as_bytes()).await.is_ok() {
            let _ = tls.write_all(body).await;
        }
    });
    format!("https://localhost:{}/api/v4", address.port())
}

#[tokio::test]
async fn scoped_http_requires_and_accepts_the_configured_self_signed_ca() {
    let CertifiedKey { cert, signing_key } =
        generate_simple_self_signed(vec!["localhost".to_owned()]).expect("certificate generates");

    let untrusted_origin = start_tls_server(cert.der().clone(), signing_key.serialize_der()).await;
    let untrusted_client =
        ScopedHttpClient::build(&instance(untrusted_origin, None)).expect("TLS client builds");
    let error = untrusted_client
        .get("/version", &[], HeaderMap::new(), &CancellationToken::new())
        .await
        .expect_err("self-signed certificate is untrusted by default");
    assert_eq!(error_code(error), "provider.tls-failed");

    let trusted_origin = start_tls_server(cert.der().clone(), signing_key.serialize_der()).await;
    let mut ca_file = NamedTempFile::new().expect("CA file creates");
    std::io::Write::write_all(&mut ca_file, cert.pem().as_bytes()).expect("CA PEM writes");
    let trusted_client = ScopedHttpClient::build(&instance(
        trusted_origin,
        Some(ca_file.path().to_string_lossy().into_owned()),
    ))
    .expect("custom CA client builds");
    let response = trusted_client
        .get("/version", &[], HeaderMap::new(), &CancellationToken::new())
        .await
        .expect("configured custom CA is trusted");
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, br#"{"version":"18.2.0"}"#);
}

fn github_query() -> ProviderRepositoryQuery {
    ProviderRepositoryQuery {
        search: String::new(),
        visibility: None,
        namespace: None,
        archived: ProviderArchivedFilter::All,
        sort: ProviderRepositorySort::Updated,
        direction: ProviderRepositoryDirection::Desc,
        page_size: 25,
    }
}

fn github_repository_fixture(
    id: u64,
    full_name: &str,
    visibility: &str,
    archived: bool,
    fork: bool,
    permission: ProviderPermission,
) -> serde_json::Value {
    let (owner, name) = full_name.split_once('/').expect("fixture has owner/name");
    let push = matches!(
        permission,
        ProviderPermission::Write | ProviderPermission::Admin
    );
    let admin = permission == ProviderPermission::Admin;
    serde_json::json!({
        "id": id,
        "name": name,
        "full_name": format!("{owner}/{name}"),
        "owner": { "login": owner },
        "html_url": format!("https://github.com/{owner}/{name}"),
        "clone_url": format!("https://github.com/{owner}/{name}.git"),
        "ssh_url": format!("git@github.com:{owner}/{name}.git"),
        "default_branch": "main",
        "visibility": visibility,
        "private": visibility == "private",
        "archived": archived,
        "fork": fork,
        "permissions": { "pull": true, "push": push, "admin": admin },
        "updated_at": "2026-07-18T12:30:00Z",
        "unknown_future_field": { "must": "be ignored" },
        "temp_clone_token": "must-never-leave-the-adapter"
    })
}

#[tokio::test]
async fn github_authenticates_and_maps_account_affiliated_repositories() {
    let server = MockServer::start_async().await;
    let user = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/user")
                .header("authorization", "Bearer github-test-token")
                .header("accept", "application/vnd.github+json")
                .header("x-github-api-version", "2026-03-10")
                .header("user-agent", "Git-Ramus/0.1");
            then.status(200).json_body(serde_json::json!({
                "id": 7,
                "login": "octo",
                "name": "Octo Cat",
                "avatar_url": "https://avatars.githubusercontent.com/u/7",
                "unknown": true
            }));
        })
        .await;
    let global_search = server
        .mock_async(|when, then| {
            when.method(GET).path("/search/repositories");
            then.status(200).json_body(serde_json::json!({
                "items": [github_repository_fixture(
                    999,
                    "unrelated/public-skill",
                    "public",
                    false,
                    false,
                    ProviderPermission::Read
                )]
            }));
        })
        .await;
    let next_link = format!("<{}>; rel=\"next\"", server.url("/user/repos?page=2"));
    let repositories = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/user/repos")
                .header("authorization", "Bearer github-test-token")
                .header("accept", "application/vnd.github+json")
                .header("x-github-api-version", "2026-03-10")
                .query_param("affiliation", "owner,collaborator,organization_member")
                .query_param("visibility", "all")
                .query_param("sort", "updated")
                .query_param("direction", "desc")
                .query_param("per_page", "100")
                .query_param("page", "1");
            then.status(200)
                .header("link", next_link)
                .header("x-ratelimit-limit", "5000")
                .header("x-ratelimit-remaining", "4999")
                .header("x-ratelimit-reset", "1784390400")
                .json_body(serde_json::json!([
                    github_repository_fixture(
                        41,
                        "octo/private-skill",
                        "private",
                        false,
                        false,
                        ProviderPermission::Write
                    ),
                    github_repository_fixture(
                        42,
                        "skills-org/organization-skill",
                        "private",
                        false,
                        false,
                        ProviderPermission::Admin
                    ),
                    github_repository_fixture(
                        43,
                        "octo/archived-skill",
                        "public",
                        true,
                        false,
                        ProviderPermission::Read
                    ),
                    github_repository_fixture(
                        44,
                        "octo/forked-skill",
                        "public",
                        false,
                        true,
                        ProviderPermission::Read
                    )
                ]));
        })
        .await;

    let client = ScopedHttpClient::for_test_http(&server.base_url()).expect("client builds");
    let provider = GithubProvider;
    let identity = provider
        .authenticate_account(&client, "github-test-token")
        .await
        .expect("account authenticates");
    assert_eq!(identity.provider_user_id, "7");
    assert_eq!(identity.username, "octo");
    assert_eq!(identity.display_name.as_deref(), Some("Octo Cat"));

    let cancellation = CancellationToken::new();
    let page = provider
        .list_repositories(
            AdapterAccountContext {
                client: &client,
                secret: "github-test-token",
                cancellation: &cancellation,
            },
            AdapterListRequest {
                query: github_query(),
                cursor: None,
            },
        )
        .await
        .expect("repositories map");

    assert_eq!(page.items.len(), 4);
    assert_eq!(page.items[0].repository_id, "41");
    assert_eq!(page.items[0].full_name, "octo/private-skill");
    assert_eq!(page.items[0].visibility, ProviderVisibility::Private);
    assert_eq!(page.items[0].permission, ProviderPermission::Write);
    assert_eq!(page.items[1].permission, ProviderPermission::Admin);
    assert!(page.items[2].archived);
    assert!(page.items[3].fork);
    assert_eq!(page.next_cursor, Some(AdapterCursor::Page(2)));
    assert_eq!(page.rate_limit.expect("rate limit").remaining, Some(4999));
    user.assert_calls_async(1).await;
    repositories.assert_calls_async(1).await;
    global_search.assert_calls_async(0).await;
}

#[tokio::test]
async fn github_get_repository_and_status_mapping_preserve_private_existence() {
    let server = MockServer::start_async().await;
    let success = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/repos/octo/private-skill")
                .header("authorization", "Bearer github-test-token");
            then.status(200).json_body(github_repository_fixture(
                41,
                "octo/private-skill",
                "private",
                false,
                false,
                ProviderPermission::Write,
            ));
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/repos/octo/forbidden");
            then.status(403).header("x-ratelimit-remaining", "12");
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/repos/octo/rate-limited");
            then.status(403)
                .header("x-ratelimit-remaining", "0")
                .header("retry-after", "3");
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(GET).path("/repos/octo/hidden");
            then.status(404);
        })
        .await;
    let client = ScopedHttpClient::for_test_http(&server.base_url()).expect("client builds");
    let provider = GithubProvider;
    let cancellation = CancellationToken::new();

    let repository = provider
        .get_repository(
            AdapterAccountContext {
                client: &client,
                secret: "github-test-token",
                cancellation: &cancellation,
            },
            RemoteRepositoryIdentity::Path {
                path: "octo/private-skill".to_owned(),
            },
        )
        .await
        .expect("repository verifies");
    assert_eq!(repository.repository_id, "41");
    success.assert_calls_async(1).await;

    for (name, expected) in [
        ("forbidden", "provider.permission-insufficient"),
        ("rate-limited", "provider.rate-limited"),
        ("hidden", "provider.permission-insufficient"),
    ] {
        let error = provider
            .get_repository(
                AdapterAccountContext {
                    client: &client,
                    secret: "github-test-token",
                    cancellation: &cancellation,
                },
                RemoteRepositoryIdentity::Path {
                    path: format!("octo/{name}"),
                },
            )
            .await
            .expect_err("status maps to a redacted Provider error");
        assert_eq!(error_code(error), expected);
    }
}

#[tokio::test]
async fn github_maps_authentication_and_invalid_json_errors() {
    let unauthorized_server = MockServer::start_async().await;
    unauthorized_server
        .mock_async(|when, then| {
            when.method(GET).path("/user");
            then.status(401);
        })
        .await;
    let client =
        ScopedHttpClient::for_test_http(&unauthorized_server.base_url()).expect("client builds");
    let error = GithubProvider
        .authenticate_account(&client, "bad-token")
        .await
        .expect_err("401 rejects authentication");
    assert_eq!(error_code(error), "provider.authentication-required");

    let invalid_server = MockServer::start_async().await;
    invalid_server
        .mock_async(|when, then| {
            when.method(GET).path("/user/repos");
            then.status(200).body("not-json");
        })
        .await;
    let client =
        ScopedHttpClient::for_test_http(&invalid_server.base_url()).expect("client builds");
    let cancellation = CancellationToken::new();
    let error = GithubProvider
        .list_repositories(
            AdapterAccountContext {
                client: &client,
                secret: "github-test-token",
                cancellation: &cancellation,
            },
            AdapterListRequest {
                query: github_query(),
                cursor: None,
            },
        )
        .await
        .expect_err("malformed JSON is normalized");
    assert_eq!(error_code(error), "provider.response-invalid");
}

#[tokio::test]
async fn github_rejects_cross_origin_pagination_links() {
    let target = MockServer::start_async().await;
    let source = MockServer::start_async().await;
    source
        .mock_async(|when, then| {
            when.method(GET).path("/user/repos");
            then.status(200)
                .header(
                    "link",
                    format!("<{}>; rel=\"next\"", target.url("/capture?page=2")),
                )
                .json_body(serde_json::json!([]));
        })
        .await;
    let target_mock = target
        .mock_async(|when, then| {
            when.method(GET).path("/capture");
            then.status(200);
        })
        .await;
    let client = ScopedHttpClient::for_test_http(&source.base_url()).expect("client builds");
    let cancellation = CancellationToken::new();

    let error = GithubProvider
        .list_repositories(
            AdapterAccountContext {
                client: &client,
                secret: "github-test-token",
                cancellation: &cancellation,
            },
            AdapterListRequest {
                query: github_query(),
                cursor: None,
            },
        )
        .await
        .expect_err("cross-origin pagination metadata is rejected");

    assert_eq!(error_code(error), "provider.response-invalid");
    target_mock.assert_calls_async(0).await;
}

fn gitlab_project_fixture(
    root_url: &str,
    id: u64,
    full_name: &str,
    visibility: &str,
    archived: bool,
    fork: bool,
    access: (Option<u64>, Option<u64>),
) -> serde_json::Value {
    let path = full_name
        .rsplit_once('/')
        .map_or(full_name, |(_, path)| path);
    let project_access = access
        .0
        .map(|access_level| serde_json::json!({ "access_level": access_level }));
    let group_access = access
        .1
        .map(|access_level| serde_json::json!({ "access_level": access_level }));
    let host = reqwest::Url::parse(root_url)
        .expect("fixture root URL parses")
        .host_str()
        .expect("fixture has host")
        .to_owned();
    serde_json::json!({
        "id": id,
        "name": if path == "skill-set" { "Skill Set" } else { path },
        "path": path,
        "path_with_namespace": full_name,
        "default_branch": "main",
        "visibility": visibility,
        "ssh_url_to_repo": format!("git@{host}:{full_name}.git"),
        "http_url_to_repo": format!("{root_url}/{full_name}.git"),
        "web_url": format!("{root_url}/{full_name}"),
        "archived": archived,
        "forked_from_project": if fork { serde_json::json!({ "id": 1 }) } else { serde_json::Value::Null },
        "permissions": { "project_access": project_access, "group_access": group_access },
        "last_activity_at": "2026-07-19T00:00:00Z",
        "unknown_future_field": [1, 2, 3]
    })
}

#[tokio::test]
async fn gitlab_validates_relative_root_and_authenticates_with_a_pat() {
    let server = MockServer::start_async().await;
    let version = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/gitlab/api/v4/version")
                .header("accept", "application/json")
                .header("user-agent", "Git-Ramus/0.1")
                .header_missing("private-token");
            then.status(200).json_body(serde_json::json!({
                "version": "18.2.0-ee",
                "revision": "ignored"
            }));
        })
        .await;
    let user = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/gitlab/api/v4/user")
                .header("private-token", "gitlab-test-token")
                .header("accept", "application/json");
            then.status(200).json_body(serde_json::json!({
                "id": 17,
                "username": "tempest",
                "name": "Yozora Tempest",
                "avatar_url": server.url("/gitlab/uploads/avatar.png"),
                "email": "must-not-leave-adapter@example.test"
            }));
        })
        .await;
    let client = ScopedHttpClient::for_test_http(&server.url("/gitlab/api/v4"))
        .expect("relative-root client builds");
    let provider = GitlabProvider;

    let metadata = provider
        .validate_instance(&client)
        .await
        .expect("instance validates");
    assert_eq!(metadata.server_version.as_deref(), Some("18.2.0-ee"));
    let identity = provider
        .authenticate_account(&client, "gitlab-test-token")
        .await
        .expect("account authenticates");
    assert_eq!(identity.provider_user_id, "17");
    assert_eq!(identity.username, "tempest");
    version.assert_calls_async(1).await;
    user.assert_calls_async(1).await;
}

#[tokio::test]
async fn gitlab_lists_only_membership_projects_across_relative_root_pages() {
    let server = MockServer::start_async().await;
    let root_url = server.url("/gitlab");
    let page_one_link = format!(
        "<{}>; rel=\"next\"",
        server.url("/gitlab/api/v4/projects?membership=true&page=2")
    );
    let first_page = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/gitlab/api/v4/projects")
                .header("private-token", "gitlab-test-token")
                .query_param("membership", "true")
                .query_param("simple", "true")
                .query_param("per_page", "100")
                .query_param("page", "1")
                .query_param("order_by", "last_activity_at")
                .query_param("sort", "asc")
                .query_param("search", "skill");
            then.status(200)
                .header("link", page_one_link)
                .header("x-next-page", "2")
                .header("ratelimit-limit", "2000")
                .header("ratelimit-remaining", "1999")
                .header("ratelimit-reset", "1784419200")
                .json_body(serde_json::json!([
                    gitlab_project_fixture(
                        &root_url,
                        42,
                        "group/subgroup/skill-set",
                        "internal",
                        false,
                        false,
                        (Some(30), None)
                    ),
                    gitlab_project_fixture(
                        &root_url,
                        43,
                        "tempest/private-skill",
                        "private",
                        false,
                        false,
                        (Some(20), Some(40))
                    )
                ]));
        })
        .await;
    let second_page = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/gitlab/api/v4/projects")
                .query_param("membership", "true")
                .query_param("simple", "true")
                .query_param("per_page", "100")
                .query_param("page", "2")
                .query_param("order_by", "last_activity_at")
                .query_param("sort", "asc")
                .query_param("search", "skill");
            then.status(200)
                .header("x-next-page", "")
                .json_body(serde_json::json!([
                    gitlab_project_fixture(
                        &root_url,
                        44,
                        "group/archived-skill",
                        "public",
                        true,
                        false,
                        (Some(10), None)
                    ),
                    gitlab_project_fixture(
                        &root_url,
                        45,
                        "group/forked-skill",
                        "private",
                        false,
                        true,
                        (Some(20), None)
                    )
                ]));
        })
        .await;
    let unrelated = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/gitlab/api/v4/projects")
                .query_param("membership", "false");
            then.status(200)
                .json_body(serde_json::json!([gitlab_project_fixture(
                    &root_url,
                    999,
                    "unrelated/public-skill",
                    "public",
                    false,
                    false,
                    (Some(10), None)
                )]));
        })
        .await;
    let client = ScopedHttpClient::for_test_http(&server.url("/gitlab/api/v4"))
        .expect("relative-root client builds");
    let provider = GitlabProvider;
    let cancellation = CancellationToken::new();
    let mut query = github_query();
    query.search = "skill".to_owned();
    query.sort = ProviderRepositorySort::Updated;
    query.direction = ProviderRepositoryDirection::Asc;

    let first = provider
        .list_repositories(
            AdapterAccountContext {
                client: &client,
                secret: "gitlab-test-token",
                cancellation: &cancellation,
            },
            AdapterListRequest {
                query: query.clone(),
                cursor: None,
            },
        )
        .await
        .expect("first page maps");
    assert_eq!(first.items[0].full_name, "group/subgroup/skill-set");
    assert_eq!(first.items[0].namespace, "group/subgroup");
    assert_eq!(first.items[0].name, "Skill Set");
    assert_eq!(first.items[0].visibility, ProviderVisibility::Internal);
    assert_eq!(first.items[0].permission, ProviderPermission::Write);
    assert_eq!(first.items[1].permission, ProviderPermission::Admin);
    assert_eq!(first.next_cursor, Some(AdapterCursor::Page(2)));
    assert_eq!(first.rate_limit.expect("rate limit").remaining, Some(1999));
    assert!(first.items[0].web_url.starts_with(&root_url));

    let second = provider
        .list_repositories(
            AdapterAccountContext {
                client: &client,
                secret: "gitlab-test-token",
                cancellation: &cancellation,
            },
            AdapterListRequest {
                query,
                cursor: Some(AdapterCursor::Page(2)),
            },
        )
        .await
        .expect("second page maps");
    assert!(second.items[0].archived);
    assert!(second.items[1].fork);
    assert!(second.next_cursor.is_none());
    first_page.assert_calls_async(1).await;
    second_page.assert_calls_async(1).await;
    unrelated.assert_calls_async(0).await;
}

#[tokio::test]
async fn gitlab_get_project_and_errors_use_stable_privacy_preserving_codes() {
    let server = MockServer::start_async().await;
    let root_url = server.url("/gitlab");
    let success = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/gitlab/api/v4/projects/group%2Fsubgroup%2Fskill-set")
                .header("private-token", "gitlab-test-token");
            then.status(200).json_body(gitlab_project_fixture(
                &root_url,
                42,
                "group/subgroup/skill-set",
                "internal",
                false,
                false,
                (Some(30), None),
            ));
        })
        .await;
    for (project_id, status) in [(403_u64, 403), (404, 404)] {
        server
            .mock_async(move |when, then| {
                when.method(GET)
                    .path(format!("/gitlab/api/v4/projects/{project_id}"));
                then.status(status);
            })
            .await;
    }
    server
        .mock_async(|when, then| {
            when.method(GET).path("/gitlab/api/v4/projects/429");
            then.status(429).header("retry-after", "4");
        })
        .await;
    let client =
        ScopedHttpClient::for_test_http(&server.url("/gitlab/api/v4")).expect("client builds");
    let cancellation = CancellationToken::new();
    let provider = GitlabProvider;

    let repository = provider
        .get_repository(
            AdapterAccountContext {
                client: &client,
                secret: "gitlab-test-token",
                cancellation: &cancellation,
            },
            RemoteRepositoryIdentity::Path {
                path: "group/subgroup/skill-set".to_owned(),
            },
        )
        .await
        .expect("nested project verifies");
    assert_eq!(repository.repository_id, "42");
    success.assert_calls_async(1).await;

    for (repository_id, expected) in [
        ("403", "provider.permission-insufficient"),
        ("404", "provider.permission-insufficient"),
        ("429", "provider.rate-limited"),
    ] {
        let error = provider
            .get_repository(
                AdapterAccountContext {
                    client: &client,
                    secret: "gitlab-test-token",
                    cancellation: &cancellation,
                },
                RemoteRepositoryIdentity::Id {
                    repository_id: repository_id.to_owned(),
                },
            )
            .await
            .expect_err("error is normalized");
        assert_eq!(error_code(error), expected);
    }
}

#[tokio::test]
async fn gitlab_maps_authentication_malformed_json_and_cross_origin_links() {
    let unauthorized = MockServer::start_async().await;
    unauthorized
        .mock_async(|when, then| {
            when.method(GET).path("/api/v4/user");
            then.status(401);
        })
        .await;
    let client =
        ScopedHttpClient::for_test_http(&unauthorized.url("/api/v4")).expect("client builds");
    let error = GitlabProvider
        .authenticate_account(&client, "bad-token")
        .await
        .expect_err("401 rejects the PAT");
    assert_eq!(error_code(error), "provider.authentication-required");

    let target = MockServer::start_async().await;
    let source = MockServer::start_async().await;
    source
        .mock_async(|when, then| {
            when.method(GET).path("/api/v4/projects");
            then.status(200)
                .header(
                    "link",
                    format!("<{}>; rel=\"next\"", target.url("/capture?page=2")),
                )
                .json_body(serde_json::json!([]));
        })
        .await;
    let client = ScopedHttpClient::for_test_http(&source.url("/api/v4")).expect("client builds");
    let cancellation = CancellationToken::new();
    let error = GitlabProvider
        .list_repositories(
            AdapterAccountContext {
                client: &client,
                secret: "gitlab-test-token",
                cancellation: &cancellation,
            },
            AdapterListRequest {
                query: github_query(),
                cursor: None,
            },
        )
        .await
        .expect_err("cross-origin pagination is rejected");
    assert_eq!(error_code(error), "provider.response-invalid");

    let malformed = MockServer::start_async().await;
    malformed
        .mock_async(|when, then| {
            when.method(GET).path("/api/v4/projects");
            then.status(200).body("{");
        })
        .await;
    let client = ScopedHttpClient::for_test_http(&malformed.url("/api/v4")).expect("client builds");
    let error = GitlabProvider
        .list_repositories(
            AdapterAccountContext {
                client: &client,
                secret: "gitlab-test-token",
                cancellation: &cancellation,
            },
            AdapterListRequest {
                query: github_query(),
                cursor: None,
            },
        )
        .await
        .expect_err("malformed JSON is normalized");
    assert_eq!(error_code(error), "provider.response-invalid");
}

struct ServiceFakeProvider {
    instance_id: Mutex<String>,
}

impl ServiceFakeProvider {
    fn new() -> Self {
        Self {
            instance_id: Mutex::new(String::new()),
        }
    }

    fn set_instance_id(&self, instance_id: &str) {
        *self.instance_id.lock().expect("instance ID lock") = instance_id.to_owned();
    }
}

impl RepositoryDiscoveryProvider for ServiceFakeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gitlab
    }

    fn validate_instance<'a>(
        &'a self,
        _client: &'a ScopedHttpClient,
    ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
        Box::pin(async {
            Ok(InstanceMetadata {
                server_version: Some("18.2.0-test".to_owned()),
            })
        })
    }

    fn authenticate_account<'a>(
        &'a self,
        _client: &'a ScopedHttpClient,
        _secret: &'a str,
    ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
        Box::pin(async {
            Ok(AccountIdentity {
                provider_user_id: "provider-user-7".to_owned(),
                username: "tempest".to_owned(),
                display_name: Some("Yozora Tempest".to_owned()),
                avatar_url: None,
            })
        })
    }

    fn list_repositories<'a>(
        &'a self,
        _context: AdapterAccountContext<'a>,
        _request: AdapterListRequest,
    ) -> BoxFuture<'a, Result<AdapterPage, AppError>> {
        Box::pin(async {
            Ok(AdapterPage {
                items: Vec::new(),
                next_cursor: None,
                rate_limit: None,
            })
        })
    }

    fn get_repository<'a>(
        &'a self,
        _context: AdapterAccountContext<'a>,
        identity: RemoteRepositoryIdentity,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
        let instance_id = self.instance_id.lock().expect("instance ID lock").clone();
        Box::pin(async move {
            let (repository_id, full_name) = match identity {
                RemoteRepositoryIdentity::Id { repository_id } if repository_id == "42" => {
                    (repository_id, "group/skill".to_owned())
                }
                RemoteRepositoryIdentity::Path { path } if path == "group/skill" => {
                    ("42".to_owned(), path)
                }
                _ => return Err(AppError::NotFound("fake repository".to_owned())),
            };
            Ok(RemoteRepository {
                provider_kind: ProviderKind::Gitlab,
                instance_id,
                repository_id,
                namespace: "group".to_owned(),
                name: "skill".to_owned(),
                full_name: full_name.clone(),
                web_url: format!("https://gitlab.example/{full_name}"),
                https_url: format!("https://gitlab.example/{full_name}.git"),
                ssh_url: format!("git@gitlab.example:{full_name}.git"),
                default_branch: Some("main".to_owned()),
                visibility: ProviderVisibility::Private,
                archived: false,
                fork: false,
                permission: ProviderPermission::Read,
                updated_at: Utc::now(),
            })
        })
    }

    fn detect_remote(
        &self,
        instance: &NormalizedInstance,
        remote: &NormalizedRemoteUrl,
    ) -> Option<RemoteRepositoryIdentity> {
        (instance.host == remote.host).then(|| RemoteRepositoryIdentity::Path {
            path: remote.path.clone(),
        })
    }
}

#[tokio::test]
async fn service_connects_without_persisting_the_pat_and_respects_provider_enabled_state() {
    let database = Database::open_in_memory().expect("database opens");
    database
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?1,?1)",
                [Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .unwrap();
    let store = ProviderStore::new(database.clone());
    let secrets = Arc::new(MemorySecretStore::default());
    let fake = Arc::new(ServiceFakeProvider::new());
    let adapters =
        ProviderAdapterRegistry::for_test(database.clone(), ProviderKind::Gitlab, fake.clone());
    let service = ProviderService::new(store.clone(), secrets.clone(), adapters);
    let instance = service
        .create_instance(CreateInstanceInput {
            provider_kind: ProviderKind::Gitlab,
            display_name: "Private GitLab".to_owned(),
            base_url: "https://gitlab.example".to_owned(),
            custom_ca_path: None,
        })
        .await
        .unwrap();
    fake.set_instance_id(&instance.id);
    let account = service
        .connect_account(
            &instance.id,
            SensitiveString::new("glpat-integration-secret".to_owned()),
        )
        .await
        .unwrap();
    let persisted = store.get_account(&account.id).unwrap();

    assert!(!persisted.secret_ref.contains("glpat-integration-secret"));
    assert_eq!(
        secrets.get(&persisted.secret_ref).unwrap().as_deref(),
        Some("glpat-integration-secret")
    );
    database
        .with_connection(|connection| {
            connection.execute(
                "UPDATE plugin_installations SET enabled=0 WHERE plugin_id='git-ramus.provider.gitlab'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(service.validate_account(&account.id).await.is_err());
    assert_eq!(store.list_accounts(&instance.id).unwrap().len(), 1);
}

#[tokio::test]
async fn binding_matches_unlisted_remotes_and_persists_without_mutating_local_git_state() {
    let database = Database::open_in_memory().expect("database opens");
    database
        .with_connection(|connection| {
            connection.execute(
                "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?1,?1)",
                [Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .unwrap();
    let local = RepositoryRepository::new(database.clone());
    let repository = Repository::new(
        "/integration/repository",
        "repository",
        RepositoryKind::Normal,
    );
    local.create(&repository).unwrap();
    let remote = Remote {
        repository_id: repository.id.clone(),
        name: "origin".to_owned(),
        fetch_url: Some("https://gitlab.example/group/skill.git".to_owned()),
        push_url: Some("git@gitlab.example:group/skill.git".to_owned()),
    };
    local.add_remote(&remote).unwrap();
    let fake = Arc::new(ServiceFakeProvider::new());
    let adapters =
        ProviderAdapterRegistry::for_test(database.clone(), ProviderKind::Gitlab, fake.clone());
    let service = ProviderService::new(
        ProviderStore::new(database),
        Arc::new(MemorySecretStore::default()),
        adapters,
    );
    let instance = service
        .create_instance(CreateInstanceInput {
            provider_kind: ProviderKind::Gitlab,
            display_name: "Binding GitLab".to_owned(),
            base_url: "https://gitlab.example".to_owned(),
            custom_ca_path: None,
        })
        .await
        .unwrap();
    fake.set_instance_id(&instance.id);
    let account = service
        .connect_account(
            &instance.id,
            SensitiveString::new("binding-token".to_owned()),
        )
        .await
        .unwrap();

    let suggestions = service
        .match_local_remotes(
            "git-ramus.provider-center",
            &instance.id,
            &account.id,
            "d0df8130-30d9-420b-a1ce-0cbabdfca632",
        )
        .await
        .unwrap();
    assert_eq!(
        suggestions[0].status,
        ProviderBindingSuggestionStatus::Suggested
    );
    let binding = service
        .bind_remote(BindRemoteInput {
            repository_id: repository.id.clone(),
            remote_name: "origin".to_owned(),
            instance_id: instance.id,
            account_id: None,
            provider_repository_id: "42".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(binding.full_name, "group/skill");
    assert_eq!(local.get_remote(&repository.id, "origin").unwrap(), remote);
}
