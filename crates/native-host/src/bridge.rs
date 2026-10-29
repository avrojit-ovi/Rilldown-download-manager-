//! Authenticated loopback client used by the native host.
//!
//! The desktop app publishes `bridge.json` with a random port and token. This
//! module reads that file, performs one small HTTP request, and returns the
//! parsed JSON. It deliberately avoids a full HTTP client: the peer is always
//! 127.0.0.1 and responses are tiny and short-lived.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

const BRIDGE_FILE: &str = "bridge.json";
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const IO_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Deserialize)]
pub struct BridgeInfo {
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub pid: u32,
}

/// Data directory used by the desktop app for its database and bridge file.
/// `RILLDOWN_DATA_DIR` overrides the platform default (used by tests and
/// portable installs).
#[must_use]
pub fn data_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("RILLDOWN_DATA_DIR") {
        return PathBuf::from(custom);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(windows) {
        std::env::var_os("APPDATA").map_or_else(
            || home.join("AppData").join("Roaming").join("rilldown"),
            |appdata| PathBuf::from(appdata).join("rilldown"),
        )
    } else if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("rilldown")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map_or_else(|| home.join(".local").join("share"), PathBuf::from)
            .join("rilldown")
    }
}

fn read_info(path: &Path) -> Result<BridgeInfo, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("Rilldown is not running ({error})"))?;
    let info: BridgeInfo =
        serde_json::from_str(&raw).map_err(|error| format!("invalid bridge file: {error}"))?;
    if info.port == 0 || info.token.is_empty() {
        return Err("invalid bridge file: missing port or token".to_string());
    }
    Ok(info)
}

fn request(
    info: &BridgeInfo,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), String> {
    let address = SocketAddr::from(([127, 0, 0, 1], info.port));
    let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|error| format!("Rilldown is not reachable on its bridge port: {error}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .map_err(|error| format!("cannot configure bridge socket: {error}"))?;

    let encoded = match body {
        Some(value) => serde_json::to_vec(value).map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        info.token,
        encoded.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&encoded))
        .and_then(|()| stream.flush())
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|error| format!("cannot send request to Rilldown: {error}"))?;

    let mut raw = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES)
        .read_to_end(&mut raw)
        .map_err(|error| format!("cannot read Rilldown's response: {error}"))?;

    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "malformed bridge response".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "malformed bridge status line".to_string())?;
    let payload = &raw[split + 4..];
    let value: Value = if payload.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(payload)
            .map_err(|error| format!("invalid JSON from Rilldown: {error}"))?
    };
    Ok((status, value))
}

/// Ask the desktop app for its status.
#[must_use]
pub fn status() -> Value {
    let path = data_dir().join(BRIDGE_FILE);
    let info = match read_info(&path) {
        Ok(info) => info,
        Err(error) => {
            return json!({
                "connected": false,
                "appRunning": false,
                "detail": format!("{error}. Start the Rilldown desktop app."),
            });
        }
    };
    match request(&info, "GET", "/v1/ping", None) {
        Ok((200, value)) => json!({
            "connected": true,
            "appRunning": true,
            "version": value.get("version").cloned().unwrap_or(Value::Null),
            "active": value.get("active").cloned().unwrap_or(json!(0)),
            "queued": value.get("queued").cloned().unwrap_or(json!(0)),
            "detail": "Connected to Rilldown.",
        }),
        Ok((status, value)) => json!({
            "connected": false,
            "appRunning": true,
            "detail": format!("Rilldown rejected the request (HTTP {status}): {value}"),
        }),
        Err(error) => json!({
            "connected": false,
            "appRunning": false,
            "detail": error,
        }),
    }
}

/// Enqueue a download through the desktop app.
pub fn download(params: &Value) -> Result<Value, String> {
    let info = read_info(&data_dir().join(BRIDGE_FILE))
        .map_err(|error| format!("{error}. Start the Rilldown desktop app, then try again."))?;
    let (status, value) = request(&info, "POST", "/v1/downloads", Some(params))?;
    if (200..300).contains(&status) {
        Ok(value)
    } else {
        Err(value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map_or_else(
                || format!("Rilldown rejected the download (HTTP {status})"),
                str::to_string,
            ))
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn fake_bridge(token: &'static str) -> (BridgeInfo, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).unwrap_or_default();
            let text = String::from_utf8_lossy(&raw).to_string();
            let authorized = text.contains(&format!("Authorization: Bearer {token}"));
            let body = if authorized {
                r#"{"ok":true,"version":"0.1.0"}"#
            } else {
                r#"{"ok":false}"#
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            text
        });
        (
            BridgeInfo {
                port,
                token: token.to_string(),
                pid: 0,
            },
            handle,
        )
    }

    #[test]
    fn reads_version_over_the_loopback_bridge() {
        let (info, server) = fake_bridge("secret-token");
        let (status, value) = request(&info, "GET", "/v1/ping", None).unwrap();
        assert_eq!(status, 200);
        assert_eq!(value["version"], "0.1.0");
        let request_text = server.join().unwrap();
        assert!(request_text.starts_with("GET /v1/ping HTTP/1.1"));
        assert!(request_text.contains("Authorization: Bearer secret-token"));
    }

    #[test]
    fn missing_bridge_file_is_an_actionable_error() {
        let directory = tempfile::tempdir().unwrap();
        let error = read_info(&directory.path().join(BRIDGE_FILE)).unwrap_err();
        assert!(error.contains("not running"));
    }

    #[test]
    fn rejects_a_bridge_file_without_a_token() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(BRIDGE_FILE);
        std::fs::write(&path, r#"{"port":8080,"token":""}"#).unwrap();
        assert!(read_info(&path).is_err());
    }
}
