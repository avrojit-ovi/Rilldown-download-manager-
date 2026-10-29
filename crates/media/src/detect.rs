//! URL/content-type detection and a dependency-free HLS master playlist parser.

use serde::{Deserialize, Serialize};
use url::Url;

/// Broad media shape, used to choose a download strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamFormat {
    /// HTTP Live Streaming manifest.
    Hls,
    /// MPEG-DASH manifest.
    Dash,
    /// A direct video file.
    ProgressiveVideo,
    /// A direct audio file.
    ProgressiveAudio,
    /// Not recognizably media.
    Unknown,
}

impl StreamFormat {
    /// Whether this format requires segment planning and a final merge.
    #[must_use]
    pub const fn is_manifest(self) -> bool {
        matches!(self, Self::Hls | Self::Dash)
    }
}

/// A candidate media URL discovered by the extension or a page scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaCandidate {
    pub url: Url,
    pub format: StreamFormat,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

impl MediaCandidate {
    /// Build a candidate, inferring the format from the URL.
    #[must_use]
    pub fn new(url: Url) -> Self {
        let format = detect_format(&url, None);
        Self {
            url,
            format,
            content_type: None,
            label: None,
        }
    }
}

/// Infer a stream format from the URL and an optional content type.
#[must_use]
pub fn detect_format(url: &Url, content_type: Option<&str>) -> StreamFormat {
    if let Some(content_type) = content_type {
        let essence = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        match essence.as_str() {
            "application/vnd.apple.mpegurl" | "application/x-mpegurl" | "audio/mpegurl" => {
                return StreamFormat::Hls;
            }
            "application/dash+xml" => return StreamFormat::Dash,
            "video/mp4" | "video/webm" | "video/x-matroska" | "video/quicktime" | "video/mp2t" => {
                return StreamFormat::ProgressiveVideo;
            }
            "audio/mpeg" | "audio/ogg" | "audio/flac" | "audio/wav" | "audio/mp4"
            | "audio/x-m4a" => return StreamFormat::ProgressiveAudio,
            _ => {}
        }
    }

    let extension = std::path::Path::new(url.path())
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "m3u8" | "m3u" => StreamFormat::Hls,
        "mpd" => StreamFormat::Dash,
        "mp4" | "webm" | "mkv" | "mov" | "ts" => StreamFormat::ProgressiveVideo,
        "mp3" | "m4a" | "aac" | "ogg" | "flac" | "wav" => StreamFormat::ProgressiveAudio,
        _ => StreamFormat::Unknown,
    }
}

/// Whether the URL looks like playable or downloadable media.
#[must_use]
pub fn is_media_url(url: &Url) -> bool {
    detect_format(url, None) != StreamFormat::Unknown
}

/// One variant advertised by an HLS master playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlsVariant {
    pub url: Url,
    #[serde(default)]
    pub bandwidth: Option<u64>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub codecs: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl HlsVariant {
    /// Human-readable label such as `1080p · 5.2 Mbps`.
    #[must_use]
    pub fn label(&self) -> String {
        let quality = match (self.width, self.height) {
            (_, Some(height)) => format!("{height}p"),
            (Some(width), None) => format!("{width}w"),
            _ => "auto".to_string(),
        };
        match self.bandwidth {
            Some(bandwidth) if bandwidth > 0 => {
                let tenths = bandwidth / 100_000;
                format!("{quality} · {}.{} Mbps", tenths / 10, tenths % 10)
            }
            _ => quality,
        }
    }
}

/// Parse an HLS master playlist. Relative variant URLs resolve against `base`.
#[must_use]
pub fn parse_hls_master(base: &Url, playlist: &str) -> Vec<HlsVariant> {
    let mut variants = Vec::new();
    let mut pending: Option<HlsAttributes> = None;

    for line in playlist.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(parse_attributes(attributes));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some(attributes) = pending.take() else {
            continue;
        };
        let Ok(url) = base.join(line) else {
            tracing::debug!(uri = line, "ignoring unparseable HLS variant URI");
            continue;
        };
        variants.push(HlsVariant {
            url,
            bandwidth: attributes.bandwidth,
            width: attributes.width,
            height: attributes.height,
            codecs: attributes.codecs,
            name: attributes.name,
        });
    }

    variants
}

#[derive(Debug, Default)]
struct HlsAttributes {
    bandwidth: Option<u64>,
    width: Option<u32>,
    height: Option<u32>,
    codecs: Option<String>,
    name: Option<String>,
}

fn parse_attributes(input: &str) -> HlsAttributes {
    let mut attributes = HlsAttributes::default();
    for (key, value) in split_attribute_list(input) {
        match key.to_ascii_uppercase().as_str() {
            "BANDWIDTH" => {
                attributes.bandwidth = value.trim().parse::<u64>().ok();
            }
            "RESOLUTION" => {
                if let Some((width, height)) = parse_resolution(&value) {
                    attributes.width = Some(width);
                    attributes.height = Some(height);
                }
            }
            "CODECS" => {
                attributes.codecs = Some(value.trim().to_string());
            }
            "NAME" => {
                attributes.name = Some(value.trim().to_string());
            }
            _ => {}
        }
    }
    attributes
}

fn split_attribute_list(input: &str) -> Vec<(String, String)> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in input.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    parts.push(current);

    parts
        .into_iter()
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ))
        })
        .collect()
}

fn parse_resolution(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.trim().split_once('x')?;
    Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn detects_manifests_and_progressive_media() {
        assert_eq!(
            detect_format(&url("https://example.com/master.m3u8?t=1"), None),
            StreamFormat::Hls
        );
        assert_eq!(
            detect_format(&url("https://example.com/manifest.mpd"), None),
            StreamFormat::Dash
        );
        assert_eq!(
            detect_format(&url("https://example.com/clip.mp4"), None),
            StreamFormat::ProgressiveVideo
        );
        assert_eq!(
            detect_format(&url("https://example.com/track.mp3"), None),
            StreamFormat::ProgressiveAudio
        );
        assert_eq!(
            detect_format(&url("https://example.com/page.html"), None),
            StreamFormat::Unknown
        );
    }

    #[test]
    fn content_type_wins_over_extension() {
        assert_eq!(
            detect_format(
                &url("https://example.com/play"),
                Some("application/vnd.apple.mpegurl; charset=utf-8")
            ),
            StreamFormat::Hls
        );
        assert_eq!(
            detect_format(&url("https://example.com/video.bin"), Some("video/mp4")),
            StreamFormat::ProgressiveVideo
        );
    }

    #[test]
    fn parses_master_playlist_with_relative_urls() {
        let playlist = r#"
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS="avc1.42e01e,mp4a.40.2"
360p/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5200000,RESOLUTION=1920x1080,CODECS="avc1.640028,mp4a.40.2"
1080p/index.m3u8
"#;
        let variants = parse_hls_master(&url("https://cdn.example.com/hls/master.m3u8"), playlist);
        assert_eq!(variants.len(), 2);
        assert_eq!(
            variants[0].url.as_str(),
            "https://cdn.example.com/hls/360p/index.m3u8"
        );
        assert_eq!(variants[0].height, Some(360));
        assert_eq!(variants[0].codecs.as_deref(), Some("avc1.42e01e,mp4a.40.2"));
        assert_eq!(variants[1].bandwidth, Some(5_200_000));
        assert_eq!(variants[1].label(), "1080p · 5.2 Mbps");
    }

    #[test]
    fn ignores_media_playlists_and_garbage() {
        let media = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\nseg0.ts\n";
        assert_eq!(
            parse_hls_master(&url("https://example.com/media.m3u8"), media),
            Vec::new()
        );
        assert_eq!(
            parse_hls_master(&url("https://example.com/master.m3u8"), "not a playlist"),
            Vec::new()
        );
    }

    #[test]
    fn ignores_unparseable_uri() {
        let playlist = "#EXT-X-STREAM-INF:BANDWIDTH=1000\nhttp://[::1\n";
        assert_eq!(
            parse_hls_master(&url("https://example.com/master.m3u8"), playlist),
            Vec::new()
        );
    }
}
