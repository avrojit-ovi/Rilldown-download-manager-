//! Filename derivation and sanitization.
//!
//! Filenames arrive from untrusted sources: `Content-Disposition`, redirects,
//! page titles, and user input. Everything here is defensive. In particular we
//! never allow a derived name to escape the chosen directory, and we refuse
//! Windows device names even on Linux so a synced folder stays portable.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

/// Maximum filename length in bytes. Leaves room for the part suffix and temp
/// directory components while staying under common filesystem limits.
const MAX_FILENAME_BYTES: usize = 180;

/// Windows reserved device names, compared case-insensitively against the stem.
const RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitize a candidate filename.
///
/// - strips any directory component (path traversal)
/// - replaces control characters and characters invalid on Windows
/// - trims trailing dots and spaces
/// - prefixes reserved device names
/// - truncates on a UTF-8 boundary while preserving a short extension
#[must_use]
pub fn sanitize_filename(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let mut cleaned = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }

    let mut name = cleaned.trim().trim_matches('.').trim().to_string();
    if name.is_empty() {
        name = "download".to_string();
    }

    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    if RESERVED_STEMS.contains(&stem.as_str()) {
        name.insert(0, '_');
    }

    while name.ends_with('.') || name.ends_with(' ') {
        name.pop();
    }
    if name.is_empty() {
        name = "download".to_string();
    }

    truncate_preserving_extension(&name, MAX_FILENAME_BYTES)
}

/// Parse a `Content-Disposition` value and return a candidate filename.
///
/// Supports the RFC 5987 `filename*=UTF-8''...` form and quoted or unquoted
/// `filename=...` parameters. Returns the raw candidate; callers still sanitize.
#[must_use]
pub fn filename_from_content_disposition(value: &str) -> Option<String> {
    let params = parse_parameters(value);

    if let Some((_, encoded)) = params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("filename*"))
    {
        if let Some(decoded) = decode_extended_value(encoded) {
            if !decoded.trim().is_empty() {
                return Some(decoded);
            }
        }
    }

    params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("filename"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Derive a filename from the last path segment of a URL.
#[must_use]
pub fn filename_from_url(url: &Url) -> Option<String> {
    let segment = url
        .path_segments()?
        .rev()
        .find(|segment| !segment.is_empty())?;
    let decoded = percent_decode_str(segment).decode_utf8().ok()?;
    let cleaned = sanitize_filename(&decoded);
    if cleaned == "download" {
        None
    } else {
        Some(cleaned)
    }
}

/// Add an extension inferred from a MIME type when the name has none.
#[must_use]
pub fn ensure_extension(filename: &str, content_type: Option<&str>) -> String {
    if Path::new(filename).extension().is_some() {
        return filename.to_string();
    }
    let Some(extension) = content_type.and_then(extension_for_mime) else {
        return filename.to_string();
    };
    sanitize_filename(&format!("{filename}.{extension}"))
}

fn extension_for_mime(content_type: &str) -> Option<&'static str> {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "application/pdf" => Some("pdf"),
        "application/zip" => Some("zip"),
        "application/x-7z-compressed" => Some("7z"),
        "application/x-rar-compressed" | "application/vnd.rar" => Some("rar"),
        "application/x-tar" => Some("tar"),
        "application/gzip" | "application/x-gzip" => Some("gz"),
        "application/json" => Some("json"),
        "application/xml" | "text/xml" => Some("xml"),
        "text/plain" => Some("txt"),
        "text/html" => Some("html"),
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "audio/mpeg" => Some("mp3"),
        "audio/ogg" => Some("ogg"),
        "audio/flac" => Some("flac"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/mp4" | "audio/x-m4a" => Some("m4a"),
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "video/x-matroska" => Some("mkv"),
        "video/quicktime" => Some("mov"),
        "application/vnd.apple.mpegurl" | "application/x-mpegurl" => Some("m3u8"),
        "application/dash+xml" => Some("mpd"),
        "application/vnd.android.package-archive" => Some("apk"),
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => Some("exe"),
        "application/x-iso9660-image" => Some("iso"),
        _ => None,
    }
}

/// Return a non-existing path in `dir`, adding ` (n)` before the extension.
#[must_use]
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }

    let (stem, extension) = split_extension(filename);
    for counter in 1..10_000 {
        let candidate_name = if extension.is_empty() {
            format!("{stem} ({counter})")
        } else {
            format!("{stem} ({counter}).{extension}")
        };
        let path = dir.join(candidate_name);
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("{stem}-{}", uuid::Uuid::new_v4()))
}

fn parse_parameters(input: &str) -> Vec<(String, String)> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    parts.push(current);

    // The first part is the disposition type (`attachment`), not a parameter.
    parts
        .into_iter()
        .skip(1)
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some((key.to_string(), value.trim().to_string()))
        })
        .collect()
}

fn decode_extended_value(value: &str) -> Option<String> {
    let mut split = value.splitn(3, '\'');
    let charset = split.next()?.trim();
    let _language = split.next()?;
    let encoded = split.next()?;
    if !charset.is_empty() && !charset.eq_ignore_ascii_case("utf-8") {
        return None;
    }
    percent_decode_str(encoded)
        .decode_utf8()
        .ok()
        .map(String::from)
}

fn split_extension(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        Some(index) if index > 0 && index + 1 < filename.len() => {
            let extension = &filename[index + 1..];
            if extension.len() <= 16 && extension.chars().all(|c| c.is_ascii_alphanumeric()) {
                (&filename[..index], extension)
            } else {
                (filename, "")
            }
        }
        _ => (filename, ""),
    }
}

fn truncate_preserving_extension(filename: &str, max_bytes: usize) -> String {
    if filename.len() <= max_bytes {
        return filename.to_string();
    }
    let (stem, extension) = split_extension(filename);
    let reserved = if extension.is_empty() {
        0
    } else {
        extension.len() + 1
    };
    let budget = max_bytes.saturating_sub(reserved).max(1);
    let mut truncated = String::new();
    for ch in stem.chars() {
        if truncated.len() + ch.len_utf8() > budget {
            break;
        }
        truncated.push(ch);
    }
    if truncated.is_empty() {
        truncated.push('_');
    }
    if extension.is_empty() {
        truncated
    } else {
        format!("{truncated}.{extension}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_path_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(
            sanitize_filename(r"..\..\Windows\System32\cmd.exe"),
            "cmd.exe"
        );
    }

    #[test]
    fn replaces_invalid_characters() {
        assert_eq!(sanitize_filename("a<b>c:d\"e|f?g*h"), "a_b_c_d_e_f_g_h");
        assert_eq!(sanitize_filename("line\nbreak"), "line_break");
    }

    #[test]
    fn handles_windows_reserved_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("con.txt"), "_con.txt");
        assert_eq!(sanitize_filename("LPT9.log"), "_LPT9.log");
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename(" report. . "), "report");
        assert_eq!(sanitize_filename(".hidden."), "hidden");
    }

    #[test]
    fn keeps_unicode_on_boundaries() {
        let long = format!("{}.pdf", "রিল".repeat(200));
        let sanitized = sanitize_filename(&long);
        assert!(sanitized.len() <= MAX_FILENAME_BYTES);
        assert!(
            Path::new(&sanitized)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        );
        assert!(sanitized.is_char_boundary(sanitized.len()));
    }

    #[test]
    fn parses_plain_content_disposition() {
        assert_eq!(
            filename_from_content_disposition("attachment; filename=\"report final.pdf\""),
            Some("report final.pdf".to_string())
        );
        assert_eq!(
            filename_from_content_disposition("attachment;filename=photo.jpg"),
            Some("photo.jpg".to_string())
        );
    }

    #[test]
    fn parses_rfc5987_content_disposition() {
        assert_eq!(
            filename_from_content_disposition(
                "attachment; filename*=UTF-8''na%C3%AFve%20file%20%E0%A6%AC%E0%A6%BE%E0%A6%82%E0%A6%B2%E0%A6%BE.txt"
            ),
            Some("naïve file বাংলা.txt".to_string())
        );
    }

    #[test]
    fn prefers_extended_filename() {
        assert_eq!(
            filename_from_content_disposition(
                "attachment; filename=\"fallback.txt\"; filename*=UTF-8''real%20name.txt"
            ),
            Some("real name.txt".to_string())
        );
    }

    #[test]
    fn derives_from_url() {
        let url = Url::parse("https://example.com/files/My%20Report.pdf?token=secret").unwrap();
        assert_eq!(filename_from_url(&url), Some("My Report.pdf".to_string()));
        assert_eq!(
            filename_from_url(&Url::parse("https://example.com/").unwrap()),
            None
        );
        assert_eq!(
            filename_from_url(&Url::parse("https://example.com/a/").unwrap()),
            Some("a".to_string())
        );
    }

    #[test]
    fn adds_mime_extension() {
        assert_eq!(
            ensure_extension("download", Some("application/pdf")),
            "download.pdf"
        );
        assert_eq!(
            ensure_extension("already.zip", Some("application/pdf")),
            "already.zip"
        );
    }

    #[test]
    fn unique_path_avoids_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_path(dir.path(), "file.zip");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_path(dir.path(), "file.zip");
        assert_eq!(second.file_name().unwrap(), "file (1).zip");
    }
}
