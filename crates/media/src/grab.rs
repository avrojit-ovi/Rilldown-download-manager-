//! Site/HLS/DASH extraction through `yt-dlp`.
//!
//! `yt-dlp` handles format selection, manifests, encrypted segments, and site
//! extractors far more faithfully than a hand-written downloader. Rilldown owns
//! the process lifecycle, progress parsing, cancellation, and the final path; the
//! tool does the media work. `ffmpeg` is used by the tool for muxing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use url::Url;

use crate::error::{MediaError, MediaResult};
use crate::tools::{Tool, Tools};

const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
const PROGRESS_PREFIX: &str = "RILLDOWN|";

/// One selectable format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFormat {
    pub id: String,
    pub label: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub filesize: Option<u64>,
    pub note: Option<String>,
}

/// Result of inspecting a page or manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub title: String,
    pub duration_seconds: Option<u64>,
    pub thumbnail: Option<String>,
    pub uploader: Option<String>,
    pub webpage_url: Option<String>,
    pub formats: Vec<MediaFormat>,
}

/// Progress emitted while a grab runs.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GrabEvent {
    Progress {
        percent: Option<f64>,
        downloaded_bytes: Option<u64>,
        total_bytes: Option<u64>,
        speed_bps: Option<u64>,
        eta_seconds: Option<u64>,
    },
    Stage {
        stage: String,
    },
    Log {
        message: String,
    },
}

/// A grab request.
#[derive(Debug, Clone)]
pub struct GrabRequest {
    pub url: Url,
    pub format_id: Option<String>,
    pub output_dir: PathBuf,
    pub filename: Option<String>,
    pub referrer: Option<String>,
}

/// A finished grab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrabOutcome {
    pub path: PathBuf,
    pub title: String,
}

/// Inspect a URL and list its formats.
pub async fn probe(url: &Url, tools: &Tools, cancel: &Arc<AtomicBool>) -> MediaResult<MediaInfo> {
    let binary = tools
        .locate(Tool::YtDlp)
        .ok_or_else(|| MediaError::MissingTool("yt-dlp is not installed yet".to_string()))?;
    let mut command = Command::new(binary);
    command
        .arg("--no-warnings")
        .arg("--no-playlist")
        .arg("--no-colors")
        .arg("-J")
        .arg(url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let output = tokio::time::timeout(PROBE_TIMEOUT, async {
        let child = command.spawn()?;
        let output = child.wait_with_output().await?;
        Ok::<_, std::io::Error>(output)
    })
    .await
    .map_err(|_| MediaError::ToolFailed {
        tool: "yt-dlp".to_string(),
        message: "inspection timed out".to_string(),
    })??;

    if cancel.load(Ordering::SeqCst) {
        return Err(MediaError::Cancelled);
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr)
            .lines()
            .last()
            .unwrap_or("inspection failed")
            .to_string();
        return Err(MediaError::ToolFailed {
            tool: "yt-dlp".to_string(),
            message,
        });
    }

    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| MediaError::Parse(format!("invalid yt-dlp metadata: {error}")))?;
    Ok(media_info_from_json(&value))
}

fn media_info_from_json(value: &serde_json::Value) -> MediaInfo {
    let string = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let number = |key: &str| value.get(key).and_then(serde_json::Value::as_f64);

    let mut formats: Vec<MediaFormat> = value
        .get("formats")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| {
                    let protocol = entry
                        .get("protocol")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    !protocol.contains("mhtml") && !protocol.contains("storyboard")
                })
                .filter_map(format_from_json)
                .collect()
        })
        .unwrap_or_default();

    formats.sort_by(|left, right| {
        right
            .height
            .unwrap_or(0)
            .cmp(&left.height.unwrap_or(0))
            .then_with(|| right.filesize.unwrap_or(0).cmp(&left.filesize.unwrap_or(0)))
    });
    formats.dedup_by(|left, right| left.id == right.id);

    MediaInfo {
        title: string("title").unwrap_or_else(|| "Untitled media".to_string()),
        duration_seconds: number("duration").map(|seconds| seconds.max(0.0) as u64),
        thumbnail: string("thumbnail"),
        uploader: string("uploader").or_else(|| string("channel")),
        webpage_url: string("webpage_url"),
        formats,
    }
}

fn format_from_json(entry: &serde_json::Value) -> Option<MediaFormat> {
    let id = entry.get("format_id")?.as_str()?.to_string();
    let ext = entry
        .get("ext")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("bin")
        .to_string();
    let height = entry
        .get("height")
        .and_then(serde_json::Value::as_u64)
        .map(|value| u32::try_from(value).unwrap_or(u32::MAX));
    let width = entry
        .get("width")
        .and_then(serde_json::Value::as_u64)
        .map(|value| u32::try_from(value).unwrap_or(u32::MAX));
    let fps = entry.get("fps").and_then(serde_json::Value::as_f64);
    let video_codec = entry
        .get("vcodec")
        .and_then(serde_json::Value::as_str)
        .filter(|codec| *codec != "none")
        .map(str::to_string);
    let audio_codec = entry
        .get("acodec")
        .and_then(serde_json::Value::as_str)
        .filter(|codec| *codec != "none")
        .map(str::to_string);
    let filesize = entry
        .get("filesize")
        .or_else(|| entry.get("filesize_approx"))
        .and_then(serde_json::Value::as_u64);
    let note = entry
        .get("format_note")
        .and_then(serde_json::Value::as_str)
        .filter(|note| !note.is_empty())
        .map(str::to_string);
    let resolution = entry
        .get("resolution")
        .and_then(serde_json::Value::as_str)
        .filter(|resolution| *resolution != "audio only")
        .map(str::to_string);

    let mut label_parts: Vec<String> = Vec::new();
    if let Some(resolution) = &resolution {
        label_parts.push(resolution.clone());
    } else if audio_codec.is_some() && video_codec.is_none() {
        label_parts.push("audio".to_string());
    }
    if let Some(fps) = fps {
        label_parts.push(format!("{fps:.0}fps"));
    }
    label_parts.push(ext.to_uppercase());
    if let Some(note) = &note {
        label_parts.push(note.clone());
    }
    let label = label_parts.join(" · ");

    Some(MediaFormat {
        id,
        label,
        ext,
        resolution,
        width,
        height,
        fps,
        video_codec,
        audio_codec,
        filesize,
        note,
    })
}

/// Run a grab to completion, reporting progress as it goes.
#[allow(clippy::too_many_lines)]
pub async fn grab(
    request: &GrabRequest,
    tools: &Tools,
    cancel: Arc<AtomicBool>,
    on_event: impl Fn(GrabEvent) + Send + Sync + 'static,
) -> MediaResult<GrabOutcome> {
    let yt_dlp = tools
        .locate(Tool::YtDlp)
        .ok_or_else(|| MediaError::MissingTool("yt-dlp is not installed yet".to_string()))?;
    tokio::fs::create_dir_all(&request.output_dir).await?;

    let path_file = request
        .output_dir
        .join(format!(".rilldown-grab-{}.path", uuid_like()));
    let pattern = match &request.filename {
        Some(name) => {
            let stem = sanitize_stem(name);
            format!("{}/{}.%(ext)s", request.output_dir.display(), stem)
        }
        None => format!(
            "{}/%(title).120B [%(id)s].%(ext)s",
            request.output_dir.display()
        ),
    };

    let mut command = Command::new(yt_dlp);
    command
        .arg("--no-playlist")
        .arg("--no-colors")
        .arg("--newline")
        .arg("--no-warnings")
        .arg("--progress")
        .arg("--progress-template")
        .arg(format!(
            "download:{PROGRESS_PREFIX}%(progress._percent_str)s|%(progress.downloaded_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s"
        ))
        .arg("--print-to-file")
        .arg("after_move:filepath")
        .arg(&path_file)
        .arg("--merge-output-format")
        .arg("mp4")
        .args(["--output", &pattern])
        .args([
            "--format",
            request.format_id.as_deref().unwrap_or("bv*+ba/b"),
        ])
        .arg(request.url.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(directory) = tools
        .locate(Tool::Ffmpeg)
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        command.arg("--ffmpeg-location").arg(directory);
    }
    if let Some(referrer) = &request.referrer {
        command.arg("--referer").arg(referrer);
    }

    on_event(GrabEvent::Stage {
        stage: "starting".to_string(),
    });
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| MediaError::ToolFailed {
        tool: "yt-dlp".to_string(),
        message: "stdout was not captured".to_string(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| MediaError::ToolFailed {
        tool: "yt-dlp".to_string(),
        message: "stderr was not captured".to_string(),
    })?;

    let (sender, mut receiver) = mpsc::channel::<Line>(256);
    spawn_reader(stdout, LineKind::Stdout, sender.clone());
    spawn_reader(stderr, LineKind::Stderr, sender);

    let event_sink = Arc::new(on_event);
    let mut last_error = String::new();
    let mut saw_progress = false;
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let status = loop {
        tokio::select! {
            line = receiver.recv() => {
                match line {
                    Some(Line { kind, text }) => {
                        match kind {
                            LineKind::Stdout => {
                                if let Some(event) = parse_progress_line(&text) {
                                    saw_progress = true;
                                    event_sink(event);
                                } else if !text.trim().is_empty() {
                                    event_sink(GrabEvent::Log { message: text });
                                }
                            }
                            LineKind::Stderr => {
                                if !text.trim().is_empty() {
                                    last_error.clone_from(&text);
                                    if text.contains("[Merger]") || text.contains("[Muxer]") {
                                        event_sink(GrabEvent::Stage { stage: "merging".to_string() });
                                    }
                                    event_sink(GrabEvent::Log { message: text });
                                }
                            }
                        }
                    }
                    None => break child.wait().await?,
                }
            }
            _ = ticker.tick() => {
                if cancel.load(Ordering::SeqCst) {
                    let _ = child.kill().await;
                    let _ = tokio::fs::remove_file(&path_file).await;
                    return Err(MediaError::Cancelled);
                }
            }
        }
    };

    if saw_progress {
        event_sink(GrabEvent::Stage {
            stage: "finalizing".to_string(),
        });
    }

    if !status.success() {
        let _ = tokio::fs::remove_file(&path_file).await;
        return Err(MediaError::ToolFailed {
            tool: "yt-dlp".to_string(),
            message: if last_error.is_empty() {
                format!("exited with {}", status.code().unwrap_or(-1))
            } else {
                last_error
            },
        });
    }

    let path = tokio::fs::read_to_string(&path_file)
        .await
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .map(PathBuf::from);
    let _ = tokio::fs::remove_file(&path_file).await;
    let path =
        path.ok_or_else(|| MediaError::Parse("yt-dlp did not report a final file".to_string()))?;
    let title = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("media")
        .to_string();

    Ok(GrabOutcome { path, title })
}

#[derive(Debug, Clone, Copy)]
enum LineKind {
    Stdout,
    Stderr,
}

#[derive(Debug)]
struct Line {
    kind: LineKind,
    text: String,
}

fn spawn_reader<R>(reader: R, kind: LineKind, sender: mpsc::Sender<Line>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(text)) = lines.next_line().await {
            if sender.send(Line { kind, text }).await.is_err() {
                break;
            }
        }
    });
}

fn parse_progress_line(line: &str) -> Option<GrabEvent> {
    let rest = line.trim().strip_prefix(PROGRESS_PREFIX)?;
    let mut parts = rest.split('|');
    let percent = parts
        .next()
        .map(str::trim)
        .map(|value| value.trim_end_matches('%').trim())
        .and_then(|value| value.parse::<f64>().ok());
    let downloaded = parts.next().and_then(parse_metric_u64);
    let total = parts.next().and_then(parse_metric_u64);
    let speed = parts
        .next()
        .and_then(parse_metric_f64)
        .map(|value| value.max(0.0) as u64);
    let eta = parts.next().and_then(parse_eta);
    Some(GrabEvent::Progress {
        percent,
        downloaded_bytes: downloaded,
        total_bytes: total,
        speed_bps: speed,
        eta_seconds: eta,
    })
}

fn parse_metric_u64(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("na") {
        return None;
    }
    trimmed
        .parse::<f64>()
        .ok()
        .map(|value| value.max(0.0) as u64)
}

fn parse_metric_f64(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("na") {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

fn parse_eta(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("na") {
        return None;
    }
    let mut seconds = 0_u64;
    for part in trimmed.split(':') {
        seconds = seconds
            .saturating_mul(60)
            .saturating_add(part.parse::<u64>().ok()?);
    }
    Some(seconds)
}

fn sanitize_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            control if control.is_control() => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "media".to_string()
    } else {
        trimmed.chars().take(150).collect()
    }
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("{nanos:x}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_progress_templates() {
        let event = parse_progress_line("RILLDOWN| 42.5%|1048576|4194304|262144.0|00:12|")
            .expect("progress line");
        match event {
            GrabEvent::Progress {
                percent,
                downloaded_bytes,
                total_bytes,
                speed_bps,
                eta_seconds,
            } => {
                assert_eq!(percent, Some(42.5));
                assert_eq!(downloaded_bytes, Some(1_048_576));
                assert_eq!(total_bytes, Some(4_194_304));
                assert_eq!(speed_bps, Some(262_144));
                assert_eq!(eta_seconds, Some(12));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn handles_missing_progress_values() {
        let event = parse_progress_line("RILLDOWN|NA|NA|NA|NA|NA").unwrap();
        match event {
            GrabEvent::Progress {
                percent,
                downloaded_bytes,
                eta_seconds,
                ..
            } => {
                assert_eq!(percent, None);
                assert_eq!(downloaded_bytes, None);
                assert_eq!(eta_seconds, None);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn parses_hour_long_etas() {
        assert_eq!(parse_eta("01:02:03"), Some(3723));
        assert_eq!(parse_eta("00:07"), Some(7));
        assert_eq!(parse_eta("nonsense"), None);
    }

    #[test]
    fn extracts_formats_and_sorts_by_quality() {
        let value = json!({
            "title": "Example",
            "duration": 132.4,
            "uploader": "Someone",
            "formats": [
                {"format_id": "137", "ext": "mp4", "height": 1080, "width": 1920, "fps": 30.0,
                 "vcodec": "avc1", "acodec": "none", "filesize": 1000, "resolution": "1920x1080"},
                {"format_id": "18", "ext": "mp4", "height": 360, "width": 640,
                 "vcodec": "avc1", "acodec": "mp4a", "filesize": 200, "resolution": "640x360"},
                {"format_id": "sb0", "ext": "mhtml", "protocol": "mhtml", "height": 90},
                {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a",
                 "resolution": "audio only", "filesize": 300}
            ]
        });
        let info = media_info_from_json(&value);
        assert_eq!(info.title, "Example");
        assert_eq!(info.duration_seconds, Some(132));
        assert_eq!(info.formats.len(), 3);
        assert_eq!(info.formats[0].id, "137");
        assert_eq!(info.formats[1].id, "18");
        assert_eq!(info.formats[2].id, "140");
        assert_eq!(info.formats[2].label, "audio · M4A");
    }

    #[test]
    fn sanitizes_requested_filenames() {
        assert_eq!(sanitize_stem("bad/name: here"), "bad_name_ here");
        assert_eq!(sanitize_stem("   "), "media");
        assert_eq!(sanitize_stem("..."), "media");
    }
}
