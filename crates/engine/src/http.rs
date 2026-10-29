//! HTTP client construction and resource probing.

use std::io;
use std::time::Duration;

use reqwest::header::{
    ACCEPT_ENCODING, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_ENCODING, CONTENT_LENGTH,
    CONTENT_RANGE, CONTENT_TYPE, COOKIE, ETAG, HeaderMap, HeaderName, HeaderValue, LAST_MODIFIED,
    RANGE, REFERER,
};
use reqwest::{Client, RequestBuilder};
use url::Url;

use crate::branding::USER_AGENT;
use crate::error::{EngineError, EngineResult};
use crate::filename::filename_from_content_disposition;
use crate::types::{DownloadRecord, ProbeInfo};

/// Build a client for one download, honoring its proxy and user agent.
pub fn build_client(record: &DownloadRecord) -> EngineResult<Client> {
    let user_agent = record.user_agent.as_deref().unwrap_or(USER_AGENT);
    let mut builder = Client::builder()
        .user_agent(user_agent)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error(io::Error::other("too many redirects"));
            }
            let scheme = attempt.url().scheme().to_owned();
            match scheme.as_str() {
                "http" | "https" => attempt.follow(),
                other => attempt.error(io::Error::other(format!(
                    "redirect to unsupported scheme: {other}"
                ))),
            }
        }))
        .no_gzip()
        .no_brotli()
        .pool_max_idle_per_host(8);

    if let Some(proxy) = &record.proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|error| {
            EngineError::Other(format!("invalid proxy for this download: {error}"))
        })?);
    }

    builder.build().map_err(EngineError::from)
}

/// Apply cookies, referrer, custom headers, and URL credentials to a request.
pub fn apply_headers(
    mut request: RequestBuilder,
    record: &DownloadRecord,
) -> EngineResult<RequestBuilder> {
    for (name, value) in &record.headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            EngineError::Other(format!("invalid request header name {name:?}: {error}"))
        })?;
        let header_value = HeaderValue::from_str(value).map_err(|error| {
            EngineError::Other(format!("invalid value for header {name:?}: {error}"))
        })?;
        request = request.header(header_name, header_value);
    }

    if let Some(cookies) = &record.cookies {
        request = request.header(COOKIE, cookies);
    }
    if let Some(referrer) = &record.referrer {
        request = request.header(REFERER, referrer);
    }
    request = request.header(ACCEPT_ENCODING, "identity");

    if !record.url.username().is_empty() {
        request = request.basic_auth(
            record.url.username(),
            Some(record.url.password().unwrap_or_default()),
        );
    }

    Ok(request)
}

/// Probe a resource without downloading its body when possible.
pub async fn probe(client: &Client, record: &DownloadRecord) -> EngineResult<ProbeInfo> {
    let head = apply_headers(client.head(record.url.clone()), record)?
        .send()
        .await?;
    let mut response = if head.status().is_success() {
        head
    } else {
        apply_headers(client.get(record.url.clone()), record)?
            .header(RANGE, "bytes=0-0")
            .send()
            .await?
    };

    // Some servers (and proxies) answer HEAD with `Content-Length: 0` even when
    // the resource is not empty. Confirm with a one-byte range request before
    // treating a zero-length HEAD as an actual empty file.
    let zero_length_head = header_string(response.headers(), &CONTENT_LENGTH)
        .as_deref()
        .is_some_and(|value| value.trim() == "0");
    let advertised_ranges = header_string(response.headers(), &ACCEPT_RANGES)
        .is_some_and(|value| value.to_ascii_lowercase().contains("bytes"));
    if response.status().is_success() && zero_length_head && !advertised_ranges {
        let confirm = apply_headers(client.get(record.url.clone()), record)?
            .header(RANGE, "bytes=0-0")
            .send()
            .await?;
        if confirm.status().is_success() {
            response = confirm;
        }
    }

    let status = response.status();
    if !status.is_success() {
        return Err(EngineError::HttpStatus {
            status: status.as_u16(),
            url: response.url().to_string(),
        });
    }

    let final_url = response.url().clone();
    if !matches!(final_url.scheme(), "http" | "https") {
        return Err(EngineError::UnsupportedScheme(
            final_url.scheme().to_string(),
        ));
    }

    let headers = response.headers();
    if let Some(encoding) = header_string(headers, &CONTENT_ENCODING) {
        if !encoding.eq_ignore_ascii_case("identity") {
            return Err(EngineError::Other(format!(
                "server forced content encoding {encoding:?}; cannot verify byte ranges"
            )));
        }
    }

    let mut total = header_string(headers, &CONTENT_LENGTH).and_then(|value| value.parse().ok());
    let mut accept_ranges = header_string(headers, &ACCEPT_RANGES)
        .is_some_and(|value| value.to_ascii_lowercase().contains("bytes"));

    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        accept_ranges = true;
        if let Some((_, _, complete_length)) = header_string(headers, &CONTENT_RANGE)
            .as_deref()
            .and_then(parse_content_range)
        {
            total = complete_length;
        }
    }

    let filename = header_string(headers, &CONTENT_DISPOSITION)
        .and_then(|value| filename_from_content_disposition(&value));
    let content_type = header_string(headers, &CONTENT_TYPE);
    let etag = header_string(headers, &ETAG);
    let last_modified = header_string(headers, &LAST_MODIFIED);

    Ok(ProbeInfo {
        final_url,
        content_length: total,
        content_type,
        accept_ranges,
        etag,
        last_modified,
        filename,
    })
}

/// Parse `Content-Range: bytes 0-1023/4096` into `(start, end, total)`.
#[must_use]
pub fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let end = end.trim().parse::<u64>().ok()?;
    let total = match total.trim() {
        "*" => None,
        value => Some(value.parse::<u64>().ok()?),
    };
    Some((start, end, total))
}

/// Whether a URL is one the engine can transfer over HTTP.
#[must_use]
pub fn is_http_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

fn header_string(headers: &HeaderMap, name: &HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_range() {
        assert_eq!(
            parse_content_range("bytes 0-99/1000"),
            Some((0, 99, Some(1000)))
        );
        assert_eq!(
            parse_content_range("bytes 512-1023/*"),
            Some((512, 1023, None))
        );
        assert_eq!(parse_content_range("0-99/1000"), None);
        assert_eq!(parse_content_range("bytes nonsense"), None);
    }
}
