//! Browser native-messaging host library.
//!
//! The host is intentionally tiny: it validates frames, answers a narrow method
//! set, and forwards work to the running desktop app over the authenticated
//! loopback bridge. It never opens the download database itself.

pub mod bridge;
pub mod framing;
pub mod protocol;

use std::io::{self, BufReader, BufWriter};

use serde_json::Value;
use tracing_subscriber::EnvFilter;

pub use framing::{MAX_FRAME_BYTES, read_frame, write_frame};
pub use protocol::{PROTOCOL_VERSION, ProtocolError, Request, Response, dispatch};

/// Turn one decoded request into a response, using the bridge for app methods.
#[must_use]
pub fn handle_request(request: &Request) -> Response {
    let id = request.id.clone();
    match request.method.as_str() {
        "status" => Response::success(id, bridge::status()),
        "download" | "add_url" | "add_download" => match bridge::download(&request.params) {
            Ok(result) => Response::success(id, result),
            Err(message) => Response::error(
                id,
                if message.contains("not running") {
                    "app_unavailable"
                } else {
                    "download_rejected"
                },
                message,
            ),
        },
        _ => dispatch(request),
    }
}

/// Decode and dispatch one native-messaging payload.
#[must_use]
pub fn handle_payload(payload: &[u8]) -> Response {
    match serde_json::from_slice::<Request>(payload) {
        Ok(request) => handle_request(&request),
        Err(error) => Response::error(None, "invalid_json", format!("invalid request: {error}")),
    }
}

/// Run the stdio native-messaging loop until the browser closes the pipe.
pub fn run() -> std::process::ExitCode {
    let filter =
        EnvFilter::try_from_env("RILLDOWN_HOST_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_target(false)
        .init();

    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());

    loop {
        match read_frame(&mut reader) {
            Ok(Some(payload)) => {
                let response = handle_payload(&payload);
                let encoded = match serde_json::to_vec(&response) {
                    Ok(encoded) => encoded,
                    Err(error) => {
                        tracing::error!(%error, "failed to encode response");
                        break;
                    }
                };
                if let Err(error) = write_frame(&mut writer, &encoded) {
                    tracing::error!(%error, "failed to write response; closing");
                    break;
                }
            }
            Ok(None) => {
                tracing::debug!("browser closed the native messaging pipe");
                break;
            }
            Err(error) => {
                tracing::error!(%error, "invalid native message frame");
                break;
            }
        }
    }

    std::process::ExitCode::SUCCESS
}

/// Stable JSON shape for a host that has no app connection yet.
#[must_use]
pub fn offline_status() -> Value {
    serde_json::json!({
        "connected": false,
        "appRunning": false,
        "detail": "The Rilldown desktop app is not running."
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_methods_stay_structured() {
        let request = Request {
            id: Some(json!(1)),
            method: "no_such_method".to_string(),
            params: Value::Null,
        };
        let response = handle_request(&request);
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "unknown_method");
    }

    #[test]
    fn invalid_json_is_reported() {
        let response = handle_payload(b"{not json");
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "invalid_json");
    }
}
