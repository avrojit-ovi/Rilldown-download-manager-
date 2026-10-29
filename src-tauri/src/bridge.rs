//! Loopback bridge between the browser native-messaging host and the app.
//!
//! Browsers can only start a native host process; they cannot call Tauri IPC.
//! The app therefore listens on a random loopback port with a per-session
//! bearer token, and publishes `{port, token}` in a private file inside the data
//! directory. The host process forwards requests; it never opens the database.
//!
//! The bridge binds to 127.0.0.1 only, caps request bodies, and rejects every
//! request without the token. It is not a general-purpose HTTP server.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use rilldown_engine::{DownloadFilter, DownloadSpec, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::AppState;

const MAX_BODY_BYTES: u64 = 128 * 1024;
const BRIDGE_FILE: &str = "bridge.json";
const TOKEN_HEADER: &str = "authorization";

/// Published connection details for the native host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
}

/// A running bridge server. Dropping it stops the thread and removes the file.
pub struct BridgeHandle {
    info: BridgeInfo,
    path: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for BridgeHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BridgeHandle")
            .field("port", &self.info.port)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl BridgeHandle {
    /// Port the server is listening on.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.info.port
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Start the bridge for this app instance.
pub fn start(app: AppHandle) -> std::io::Result<BridgeHandle> {
    let server = tiny_http::Server::http(("127.0.0.1", 0))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let port = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| std::io::Error::other("bridge did not bind to an IP address"))?
        .port();

    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let info = BridgeInfo {
        port,
        token: token.clone(),
        pid: std::process::id(),
        version: rilldown_engine::branding::APP_VERSION.to_string(),
    };

    let data_dir = app.state::<AppState>().engine.config().data_dir.clone();
    std::fs::create_dir_all(&data_dir)?;
    let path = data_dir.join(BRIDGE_FILE);
    write_private(&path, &serde_json::to_vec(&info)?)?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread = std::thread::Builder::new()
        .name("rilldown-bridge".to_string())
        .spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(400)) {
                    Ok(Some(request)) => handle(&app, &token, request),
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(%error, "browser bridge stopped accepting requests");
                        break;
                    }
                }
            }
        })?;

    Ok(BridgeHandle {
        info,
        path,
        stop,
        thread: Some(thread),
    })
}

fn write_private(path: &PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn handle(app: &AppHandle, token: &str, mut request: tiny_http::Request) {
    let method = request.method().as_str().to_ascii_uppercase();
    let url = request.url().to_string();

    let authorized = request.headers().iter().any(|header| {
        header.field.equiv(TOKEN_HEADER)
            && header
                .value
                .as_str()
                .strip_prefix("Bearer ")
                .is_some_and(|candidate| constant_time_eq(candidate, token))
    });
    if !authorized {
        respond_error(
            request,
            401,
            "unauthorized",
            "missing or invalid bridge token",
        );
        return;
    }

    let mut body = String::new();
    if method == "POST" {
        match request
            .as_reader()
            .take(MAX_BODY_BYTES)
            .read_to_string(&mut body)
        {
            Ok(_) => {}
            Err(error) => {
                respond_error(
                    request,
                    400,
                    "bad_request",
                    &format!("unreadable body: {error}"),
                );
                return;
            }
        }
    }

    let path = url.split('?').next().unwrap_or_default();
    match (method.as_str(), path) {
        ("GET", "/v1/ping" | "/v1/status") => respond_json(request, 200, &status(app)),
        ("GET", "/v1/downloads") => respond_json(request, 200, &downloads(app)),
        ("POST", "/v1/downloads") => add_download(app, request, &body),
        ("GET", _) if path.starts_with("/v1/downloads/") => {
            let id = path.trim_start_matches("/v1/downloads/");
            match id.parse::<rilldown_engine::DownloadId>() {
                Ok(id) => {
                    let result = app.state::<AppState>().engine.get(id);
                    match result {
                        Ok(record) => {
                            respond_json(request, 200, &json!({ "ok": true, "download": record }));
                        }
                        Err(error) => respond_error(request, 404, error.code(), &error.to_string()),
                    }
                }
                Err(error) => respond_error(request, 400, "invalid_id", &error.to_string()),
            }
        }
        _ => respond_error(request, 404, "not_found", "unknown bridge endpoint"),
    }
}

fn status(app: &AppHandle) -> Value {
    let state = app.state::<AppState>();
    let records = state
        .engine
        .list(&DownloadFilter::default())
        .unwrap_or_default();
    let active = records
        .iter()
        .filter(|record| record.state.is_active())
        .count();
    let queued = records
        .iter()
        .filter(|record| record.state == rilldown_engine::DownloadState::Queued)
        .count();
    json!({
        "ok": true,
        "appRunning": true,
        "name": rilldown_engine::branding::APP_NAME,
        "version": rilldown_engine::branding::APP_VERSION,
        "active": active,
        "queued": queued,
        "total": records.len(),
    })
}

fn downloads(app: &AppHandle) -> Value {
    let state = app.state::<AppState>();
    let records = state
        .engine
        .list(&DownloadFilter::default())
        .unwrap_or_default();
    json!({ "ok": true, "downloads": records })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddRequest {
    url: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    referrer: Option<String>,
    #[serde(default)]
    cookies: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    connections: Option<u8>,
    #[serde(default)]
    queue_id: Option<String>,
}

fn add_download(app: &AppHandle, request: tiny_http::Request, body: &str) {
    let payload: AddRequest = match serde_json::from_str(body) {
        Ok(payload) => payload,
        Err(error) => {
            respond_error(
                request,
                400,
                "invalid_json",
                &format!("invalid request: {error}"),
            );
            return;
        }
    };
    let url = match Url::parse(&payload.url) {
        Ok(url) => url,
        Err(error) => {
            respond_error(
                request,
                400,
                "invalid_url",
                &format!("invalid URL: {error}"),
            );
            return;
        }
    };

    let state = app.state::<AppState>();
    let settings = state.settings.lock().expect("settings lock").clone();
    let mut spec = DownloadSpec::new(url, settings.download_dir.clone());
    spec.filename = payload.filename;
    spec.referrer = payload.referrer;
    spec.cookies = payload.cookies;
    spec.headers = payload.headers;
    spec.connections = payload.connections.unwrap_or(settings.connections);
    spec.queue_id = Some(
        payload
            .queue_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| rilldown_engine::DEFAULT_QUEUE_ID.to_string()),
    );
    spec.proxy.clone_from(&settings.proxy);
    spec.user_agent.clone_from(&settings.user_agent);

    let result = tauri::async_runtime::block_on(state.engine.add(spec));
    match result {
        Ok(record) => respond_json(
            request,
            201,
            &json!({
                "ok": true,
                "download": {
                    "id": record.id,
                    "filename": record.filename,
                    "state": record.state,
                }
            }),
        ),
        Err(error) => respond_error(request, 422, error.code(), &error.to_string()),
    }
}

fn respond_json(request: tiny_http::Request, status: u16, value: &Value) {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    let header = tiny_http::Header::from_bytes("Content-Type", "application/json")
        .expect("static header is valid");
    let response = tiny_http::Response::from_data(body)
        .with_status_code(status)
        .with_header(header);
    let _ = request.respond(response);
}

fn respond_error(request: tiny_http::Request, status: u16, code: &str, message: &str) {
    respond_json(
        request,
        status,
        &json!({ "ok": false, "error": { "code": code, "message": message } }),
    );
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut different = 0_u8;
    for (a, b) in left.iter().zip(right) {
        different |= a ^ b;
    }
    different == 0
}

/// The native host resolves the same file without depending on Tauri.
#[must_use]
pub fn bridge_path_for(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join(BRIDGE_FILE)
}

/// Best-effort removal of a stale bridge file at startup.
pub fn clear_stale(data_dir: &std::path::Path) {
    let path = bridge_path_for(data_dir);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(info) = serde_json::from_str::<BridgeInfo>(&raw) {
            if info.pid != std::process::id() {
                let _ = std::fs::remove_file(&path);
            }
            return;
        }
    }
    let _ = std::fs::remove_file(&path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_requires_an_exact_match() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
    }

    #[test]
    fn stale_bridge_files_are_cleared() {
        let directory = tempfile::tempdir().unwrap();
        let path = bridge_path_for(directory.path());
        std::fs::write(&path, r#"{"port":1,"token":"t","pid":0,"version":"0.1.0"}"#).unwrap();
        clear_stale(directory.path());
        assert!(!path.exists());
    }
}
