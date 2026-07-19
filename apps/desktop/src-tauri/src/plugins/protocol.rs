use tauri::http::{Method, Request, Response, StatusCode};

use crate::plugins::PluginRegistry;

pub const PLUGIN_PROTOCOL_SCHEME: &str = "git-ramus-plugin";

const PLUGIN_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

pub fn plugin_ui_url(plugin_id: &str) -> String {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        format!("http://{PLUGIN_PROTOCOL_SCHEME}.localhost/{plugin_id}/ui.html")
    } else {
        format!("{PLUGIN_PROTOCOL_SCHEME}://localhost/{plugin_id}/ui.html")
    }
}

pub fn build_plugin_response(
    registry: &PluginRegistry,
    request: &Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if request.method() != Method::GET {
        return response(
            StatusCode::METHOD_NOT_ALLOWED,
            "text/plain; charset=utf-8",
            b"method not allowed".to_vec(),
        );
    }
    let Some(plugin_id) = requested_plugin_id(request.uri().path()) else {
        return response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"not found".to_vec(),
        );
    };
    let Some(descriptor) = registry.get(plugin_id) else {
        return response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"not found".to_vec(),
        );
    };
    let Some(ui_html) = descriptor.ui_html.as_deref() else {
        return response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"not found".to_vec(),
        );
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("content-security-policy", PLUGIN_CSP)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
        .body(ui_html.as_bytes().to_vec())
        .expect("static plugin response headers are valid")
}

pub fn service_unavailable_response() -> Response<Vec<u8>> {
    response(
        StatusCode::SERVICE_UNAVAILABLE,
        "text/plain; charset=utf-8",
        b"plugin host unavailable".to_vec(),
    )
}

fn requested_plugin_id(path: &str) -> Option<&str> {
    let mut components = path.trim_start_matches('/').split('/');
    let plugin_id = components.next()?;
    if plugin_id.is_empty() || components.next() != Some("ui.html") || components.next().is_some() {
        return None;
    }
    Some(plugin_id)
}

fn response(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
        .body(body)
        .expect("static plugin response headers are valid")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tauri::http::{Request, StatusCode};
    use tempfile::tempdir;

    use crate::plugins::PluginRegistry;

    use super::{build_plugin_response, plugin_ui_url};

    fn registry_with_plugin() -> PluginRegistry {
        let directory = tempdir().expect("temp directory creates");
        let plugin = directory.path().join("git-ramus.welcome");
        fs::create_dir_all(&plugin).expect("plugin directory creates");
        fs::write(
            plugin.join("plugin.json"),
            r#"{"schemaVersion":1,"id":"git-ramus.welcome","name":"Welcome","version":"0.1.0","publisher":"git-ramus","description":"Welcome plugin","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{"ui":"ui.html"},"contributions":{"navigation":[]},"permissions":[]}"#,
        )
        .expect("manifest writes");
        fs::write(
            plugin.join("ui.html"),
            "<script>document.body.textContent = 'plugin ran'</script>",
        )
        .expect("UI writes");
        PluginRegistry::discover(directory.path()).expect("plugin discovers")
    }

    fn registry_with_backend_provider() -> PluginRegistry {
        let directory = tempdir().expect("temp directory creates");
        let plugin = directory.path().join("git-ramus.provider.gitlab");
        fs::create_dir_all(&plugin).expect("plugin directory creates");
        fs::write(
            plugin.join("plugin.json"),
            r#"{"schemaVersion":1,"id":"git-ramus.provider.gitlab","name":"GitLab Provider","version":"0.1.0","publisher":"git-ramus","description":"GitLab API adapter.","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{},"contributions":{"navigation":[],"providers":[{"providerId":"gitlab","adapterId":"git-ramus.provider.gitlab","displayName":"GitLab","icon":"gitlab","instanceModes":["cloud","selfHosted"],"capabilities":["repositoryDiscovery","customCa"]}]},"permissions":[]}"#,
        )
        .expect("manifest writes");
        PluginRegistry::discover(directory.path()).expect("provider discovers")
    }

    #[test]
    fn serves_registered_ui_with_a_network_denying_response_csp() {
        let registry = registry_with_plugin();
        let request = Request::builder()
            .uri(plugin_ui_url("git-ramus.welcome"))
            .body(Vec::new())
            .expect("request builds");

        let response = build_plugin_response(&registry, &request);

        assert_eq!(response.status(), StatusCode::OK);
        let csp = response
            .headers()
            .get("content-security-policy")
            .expect("response has CSP")
            .to_str()
            .expect("CSP is text");
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("script-src 'unsafe-inline'"));
        assert!(csp.contains("connect-src 'none'"));
        assert_eq!(
            response
                .headers()
                .get("x-content-type-options")
                .expect("nosniff header exists"),
            "nosniff"
        );
        assert_eq!(
            response.body().as_slice(),
            b"<script>document.body.textContent = 'plugin ran'</script>"
        );
    }

    #[test]
    fn rejects_unregistered_plugin_paths() {
        let registry = registry_with_plugin();
        let request = Request::builder()
            .uri(plugin_ui_url("git-ramus.missing"))
            .body(Vec::new())
            .expect("request builds");

        let response = build_plugin_response(&registry, &request);

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn rejects_ui_requests_for_backend_only_plugins() {
        let registry = registry_with_backend_provider();
        let request = Request::builder()
            .uri(plugin_ui_url("git-ramus.provider.gitlab"))
            .body(Vec::new())
            .expect("request builds");

        let response = build_plugin_response(&registry, &request);

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
