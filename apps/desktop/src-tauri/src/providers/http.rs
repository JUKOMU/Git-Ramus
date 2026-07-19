use std::error::Error as StdError;
use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, HeaderMap};
use reqwest::redirect::Policy;
use reqwest::{Certificate, Client, StatusCode, Url};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, ProviderFailure};
use crate::providers::model::ProviderInstance;

const MAX_CUSTOM_CA_BYTES: u64 = 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const MAX_ATTEMPTS: usize = 3;

#[derive(Clone, Copy)]
struct HttpLimits {
    connect_timeout: Duration,
    read_timeout: Duration,
    total_timeout: Duration,
    max_body_bytes: usize,
    retry_delays: [Duration; 2],
    retry_jitter_ms: u64,
}

impl HttpLimits {
    const fn production() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(45),
            max_body_bytes: 2 * 1024 * 1024,
            retry_delays: [Duration::from_millis(100), Duration::from_millis(250)],
            retry_jitter_ms: 25,
        }
    }

    #[cfg(any(test, debug_assertions))]
    const fn for_test(
        total_timeout: Duration,
        max_body_bytes: usize,
        retry_delays: [Duration; 2],
    ) -> Self {
        Self {
            connect_timeout: total_timeout,
            read_timeout: total_timeout,
            total_timeout,
            max_body_bytes,
            retry_delays,
            retry_jitter_ms: 0,
        }
    }
}

pub struct ScopedHttpClient {
    instance_id: String,
    origin: Url,
    client: Client,
    limits: HttpLimits,
}

impl fmt::Debug for ScopedHttpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScopedHttpClient")
            .field("max_body_bytes", &self.limits.max_body_bytes)
            .finish_non_exhaustive()
    }
}

pub struct BoundedResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub retry_after_ms: Option<u64>,
}

impl fmt::Debug for BoundedResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .field("header_count", &self.headers.len())
            .field("retry_after_ms", &self.retry_after_ms)
            .finish()
    }
}

impl ScopedHttpClient {
    pub fn build(instance: &ProviderInstance) -> Result<Self, AppError> {
        Self::build_inner(
            &instance.id,
            &instance.api_base_url,
            instance.custom_ca_path.as_deref(),
            HttpLimits::production(),
            false,
        )
    }

    #[cfg(test)]
    pub(crate) fn build_with_limits(
        instance: &ProviderInstance,
        total_timeout: Duration,
        max_body_bytes: usize,
        retry_delays: [Duration; 2],
    ) -> Result<Self, AppError> {
        Self::build_inner(
            &instance.id,
            &instance.api_base_url,
            instance.custom_ca_path.as_deref(),
            HttpLimits::for_test(total_timeout, max_body_bytes, retry_delays),
            false,
        )
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn for_test_http(api_base_url: &str) -> Result<Self, AppError> {
        Self::build_inner(
            "test-instance",
            api_base_url,
            None,
            HttpLimits::production(),
            true,
        )
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn for_test_http_with_limits(
        api_base_url: &str,
        total_timeout: Duration,
        max_body_bytes: usize,
        retry_delays: [Duration; 2],
    ) -> Result<Self, AppError> {
        Self::build_inner(
            "test-instance",
            api_base_url,
            None,
            HttpLimits::for_test(total_timeout, max_body_bytes, retry_delays),
            true,
        )
    }

    fn build_inner(
        instance_id: &str,
        api_base_url: &str,
        custom_ca_path: Option<&str>,
        limits: HttpLimits,
        allow_test_http: bool,
    ) -> Result<Self, AppError> {
        let origin = normalize_api_base_url(api_base_url, allow_test_http)?;
        let redirect_origin = origin.clone();
        let redirect_policy = Policy::custom(move |attempt| {
            if !same_origin(&redirect_origin, attempt.url()) {
                return attempt.error(RejectedRedirect);
            }
            if attempt.previous().len() > MAX_REDIRECTS {
                return attempt.error(RejectedRedirect);
            }
            attempt.follow()
        });

        let mut builder = Client::builder()
            .connect_timeout(limits.connect_timeout)
            .read_timeout(limits.read_timeout)
            .timeout(limits.total_timeout)
            .redirect(redirect_policy);
        if !allow_test_http {
            builder = builder.https_only(true);
        }
        if let Some(path) = custom_ca_path {
            builder = builder.add_root_certificate(load_custom_ca(path)?);
        }
        let client = builder
            .build()
            .map_err(|_| AppError::Provider(ProviderFailure::tls()))?;
        Ok(Self {
            instance_id: instance_id.to_owned(),
            origin,
            client,
            limits,
        })
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn is_same_origin(&self, url: &Url) -> bool {
        same_origin(&self.origin, url)
    }

    pub async fn get(
        &self,
        relative_path: &str,
        query: &[(&str, String)],
        headers: HeaderMap,
        cancellation: &CancellationToken,
    ) -> Result<BoundedResponse, AppError> {
        let url = self.resolve(relative_path)?;
        let deadline = Instant::now() + self.limits.total_timeout;
        for attempt in 0..MAX_ATTEMPTS {
            if cancellation.is_cancelled() {
                return Err(canceled());
            }
            let request = self
                .client
                .get(url.clone())
                .query(query)
                .headers(headers.clone());
            let response = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(canceled()),
                result = tokio::time::timeout_at(deadline, request.send()) => {
                    match result {
                        Ok(Ok(response)) => response,
                        Ok(Err(error)) => {
                            if is_tls_error(&error) {
                                return Err(AppError::Provider(ProviderFailure::tls()));
                            }
                            if retryable_transport(&error) && attempt + 1 < MAX_ATTEMPTS {
                                self.backoff(attempt, deadline, cancellation).await?;
                                continue;
                            }
                            return Err(map_transport_error(&error));
                        }
                        Err(_) => return Err(unreachable()),
                    }
                }
            };

            if response.status().is_redirection() {
                return Err(AppError::Provider(ProviderFailure::invalid_response()));
            }
            if matches!(
                response.status(),
                StatusCode::BAD_GATEWAY
                    | StatusCode::SERVICE_UNAVAILABLE
                    | StatusCode::GATEWAY_TIMEOUT
            ) {
                if attempt + 1 < MAX_ATTEMPTS {
                    self.backoff(attempt, deadline, cancellation).await?;
                    continue;
                }
                return Err(unreachable());
            }

            match self.read_response(response, deadline, cancellation).await {
                Ok(response) => return Ok(response),
                Err(ReadFailure::Canceled) => return Err(canceled()),
                Err(ReadFailure::Deadline) => return Err(unreachable()),
                Err(ReadFailure::Limit) => {
                    return Err(AppError::Provider(ProviderFailure::invalid_response()));
                }
                Err(ReadFailure::Transport(error)) => {
                    if is_tls_error(&error) {
                        return Err(AppError::Provider(ProviderFailure::tls()));
                    }
                    if retryable_transport(&error) && attempt + 1 < MAX_ATTEMPTS {
                        self.backoff(attempt, deadline, cancellation).await?;
                        continue;
                    }
                    return Err(map_transport_error(&error));
                }
            }
        }
        Err(unreachable())
    }

    fn resolve(&self, relative_path: &str) -> Result<Url, AppError> {
        if relative_path.is_empty()
            || relative_path.starts_with("//")
            || Url::parse(relative_path).is_ok()
            || relative_path.chars().any(char::is_control)
            || relative_path.contains(['?', '#', '\\'])
        {
            return Err(AppError::Provider(ProviderFailure::invalid_response()));
        }
        let path = relative_path.strip_prefix('/').unwrap_or(relative_path);
        if path.is_empty() || path.split('/').any(is_dot_segment) {
            return Err(AppError::Provider(ProviderFailure::invalid_response()));
        }
        let joined = self
            .origin
            .join(path)
            .map_err(|_| AppError::Provider(ProviderFailure::invalid_response()))?;
        if !same_origin(&self.origin, &joined) {
            return Err(AppError::Provider(ProviderFailure::invalid_response()));
        }
        Ok(joined)
    }

    async fn read_response(
        &self,
        response: reqwest::Response,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<BoundedResponse, ReadFailure> {
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|length| length > self.limits.max_body_bytes as u64)
        {
            return Err(ReadFailure::Limit);
        }
        let status = response.status();
        let headers = safe_response_headers(response.headers());
        let retry_after_ms = if status == StatusCode::TOO_MANY_REQUESTS {
            parse_retry_after(&headers)
        } else {
            None
        };
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(ReadFailure::Canceled),
                result = tokio::time::timeout_at(deadline, stream.next()) => {
                    match result {
                        Ok(value) => value,
                        Err(_) => return Err(ReadFailure::Deadline),
                    }
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(ReadFailure::Transport)?;
            if body
                .len()
                .checked_add(chunk.len())
                .is_none_or(|length| length > self.limits.max_body_bytes)
            {
                return Err(ReadFailure::Limit);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(BoundedResponse {
            status,
            headers,
            body,
            retry_after_ms,
        })
    }

    async fn backoff(
        &self,
        attempt: usize,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<(), AppError> {
        let base = self.limits.retry_delays[attempt.min(1)];
        let jitter = retry_jitter(self.limits.retry_jitter_ms);
        let delay = base.saturating_add(jitter);
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(canceled()),
            _ = tokio::time::sleep_until(deadline) => Err(unreachable()),
            _ = tokio::time::sleep(delay) => Ok(()),
        }
    }
}

enum ReadFailure {
    Canceled,
    Deadline,
    Limit,
    Transport(reqwest::Error),
}

#[derive(Debug)]
struct RejectedRedirect;

impl fmt::Display for RejectedRedirect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("redirect rejected")
    }
}

impl StdError for RejectedRedirect {}

fn normalize_api_base_url(value: &str, allow_test_http: bool) -> Result<Url, AppError> {
    let mut url =
        Url::parse(value).map_err(|_| AppError::Provider(ProviderFailure::invalid_response()))?;
    let scheme_allowed = url.scheme() == "https" || (allow_test_http && url.scheme() == "http");
    if !scheme_allowed
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::Provider(ProviderFailure::invalid_response()));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn is_dot_segment(segment: &str) -> bool {
    let decoded_dots = segment.to_ascii_lowercase().replace("%2e", ".");
    matches!(decoded_dots.as_str(), "." | "..")
}

fn load_custom_ca(value: &str) -> Result<Certificate, AppError> {
    let path = Path::new(value);
    let metadata = path
        .metadata()
        .map_err(|_| AppError::Provider(ProviderFailure::tls()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CUSTOM_CA_BYTES {
        return Err(AppError::Provider(ProviderFailure::tls()));
    }
    let bytes = std::fs::read(path).map_err(|_| AppError::Provider(ProviderFailure::tls()))?;
    Certificate::from_pem(&bytes)
        .or_else(|_| Certificate::from_der(&bytes))
        .map_err(|_| AppError::Provider(ProviderFailure::tls()))
}

fn safe_response_headers(source: &HeaderMap) -> HeaderMap {
    let mut safe = HeaderMap::new();
    for (name, value) in source {
        let name_text = name.as_str();
        if matches!(
            name_text,
            "content-type" | "link" | "retry-after" | "x-next-page"
        ) || name_text.starts_with("x-ratelimit-")
            || name_text.starts_with("ratelimit-")
        {
            safe.append(name.clone(), value.clone());
        }
    }
    safe
}

fn parse_retry_after(headers: &HeaderMap) -> Option<u64> {
    let value = headers.get("retry-after")?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1_000));
    }
    let retry_at = DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&Utc);
    let milliseconds = retry_at
        .signed_duration_since(Utc::now())
        .num_milliseconds();
    Some(milliseconds.max(0) as u64)
}

fn retry_jitter(maximum_ms: u64) -> Duration {
    if maximum_ms == 0 {
        return Duration::ZERO;
    }
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| u64::from(duration.subsec_nanos()));
    Duration::from_millis(nanos % (maximum_ms + 1))
}

fn is_tls_error(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(current) = source {
        let message = current.to_string().to_ascii_lowercase();
        if [
            "certificate",
            "tls",
            "rustls",
            "unknown issuer",
            "handshake failure",
        ]
        .iter()
        .any(|marker| message.contains(marker))
        {
            return true;
        }
        source = current.source();
    }
    false
}

fn retryable_transport(error: &reqwest::Error) -> bool {
    !error.is_redirect()
        && !is_tls_error(error)
        && (error.is_timeout() || error.is_connect() || error.is_body())
}

fn map_transport_error(error: &reqwest::Error) -> AppError {
    if is_tls_error(error) {
        AppError::Provider(ProviderFailure::tls())
    } else if retryable_transport(error) {
        unreachable()
    } else {
        AppError::Provider(ProviderFailure::invalid_response())
    }
}

fn canceled() -> AppError {
    AppError::Provider(ProviderFailure::canceled())
}

fn unreachable() -> AppError {
    AppError::Provider(ProviderFailure::unreachable(true))
}

#[cfg(test)]
mod tests {
    use super::ScopedHttpClient;
    use crate::providers::model::{ProviderInstance, ProviderKind};
    use chrono::Utc;
    use std::time::Duration;

    fn instance(api_base_url: &str) -> ProviderInstance {
        let now = Utc::now();
        ProviderInstance {
            id: "instance".to_owned(),
            provider_kind: ProviderKind::Gitlab,
            display_name: "GitLab".to_owned(),
            base_url: "https://gitlab.example".to_owned(),
            api_base_url: api_base_url.to_owned(),
            custom_ca_path: None,
            last_validated_at: None,
            server_version: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn build_with_limits_rejects_paths_that_escape_structured_api_routing() {
        let client = ScopedHttpClient::build_with_limits(
            &instance("https://gitlab.example/api/v4"),
            Duration::from_millis(20),
            32,
            [Duration::ZERO, Duration::ZERO],
        )
        .expect("client builds");
        for path in [
            "https://attacker.example/user",
            "//attacker.example/user",
            "../user",
            "projects?membership=false",
            "projects#fragment",
            r"projects\admin",
        ] {
            assert!(
                client.resolve(path).is_err(),
                "accepted unsafe path: {path}"
            );
        }
    }
}
