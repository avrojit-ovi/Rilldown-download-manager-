//! Native messaging protocol.
//!
//! The protocol is deliberately small and versioned. Every response echoes the
//! request id so a browser can correlate concurrent calls. The host is a thin
//! bridge; download state lives in the desktop app.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Protocol version understood by this host.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from the browser extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A response back to the extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub id: Option<Value>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl Response {
    /// Successful response.
    #[must_use]
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    /// Error response.
    #[must_use]
    pub fn error(id: Option<Value>, code: &str, message: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(ProtocolError {
                code: code.to_string(),
                message: message.into(),
            }),
        }
    }
}

/// Structured protocol error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

/// Dispatch a request without touching any I/O.
#[must_use]
pub fn dispatch(request: &Request) -> Response {
    let id = request.id.clone();
    match request.method.as_str() {
        "ping" | "get_version" => Response::success(
            id,
            json!({
                "name": "Rilldown",
                "version": env!("CARGO_PKG_VERSION"),
                "protocol": PROTOCOL_VERSION,
            }),
        ),
        "status" => Response::success(
            id,
            json!({
                "connected": false,
                "appRunning": false,
                "detail": "The Rilldown desktop app is not connected yet.",
            }),
        ),
        other => Response::error(
            id,
            "unknown_method",
            format!("unknown native host method: {other}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_reports_identity_and_protocol() {
        let request = Request {
            id: Some(json!(7)),
            method: "ping".to_string(),
            params: Value::Null,
        };
        let response = dispatch(&request);
        assert!(response.ok);
        assert_eq!(response.id, Some(json!(7)));
        let result = response.result.unwrap();
        assert_eq!(result["name"], "Rilldown");
        assert_eq!(result["protocol"], PROTOCOL_VERSION);
    }

    #[test]
    fn unknown_method_is_structured() {
        let request = Request {
            id: None,
            method: "delete_everything".to_string(),
            params: Value::Null,
        };
        let response = dispatch(&request);
        assert!(!response.ok);
        assert_eq!(response.error.unwrap().code, "unknown_method");
    }

    #[test]
    fn status_is_honest_before_app_connection() {
        let request = Request {
            id: None,
            method: "status".to_string(),
            params: Value::Null,
        };
        let response = dispatch(&request);
        assert!(response.ok);
        assert_eq!(response.result.unwrap()["connected"], false);
    }
}
