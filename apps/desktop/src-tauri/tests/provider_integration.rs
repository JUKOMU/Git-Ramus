use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chrono::Utc;
use git_ramus_desktop_lib::error::{AppError, ErrorEnvelope};
use git_ramus_desktop_lib::providers::http::ScopedHttpClient;
use git_ramus_desktop_lib::providers::model::{ProviderInstance, ProviderKind};
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
