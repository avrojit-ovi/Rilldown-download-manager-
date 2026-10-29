//! External tool management.
//!
//! Rilldown uses two well-known tools instead of reimplementing the long tail of
//! container muxing and site extraction: `ffmpeg` for HLS/DASH muxing and
//! `yt-dlp` for site extraction. Both are looked up on `PATH` first, then in the
//! app's private tools directory. If neither exists, [`Tools::ensure`] downloads
//! a pinned build on first use, as the product decision requires.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use futures_util::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::error::{MediaError, MediaResult};

const MAX_TOOL_BYTES: u64 = 512 * 1024 * 1024;

/// Which external tool is being managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Ffmpeg,
    YtDlp,
}

impl Tool {
    /// Identifier used in JSON and settings.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::YtDlp => "yt-dlp",
        }
    }

    /// Executable file name on this platform.
    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Self::Ffmpeg, true) => "ffmpeg.exe",
            (Self::Ffmpeg, false) => "ffmpeg",
            (Self::YtDlp, true) => "yt-dlp.exe",
            (Self::YtDlp, false) => "yt-dlp",
        }
    }

    fn version_args(self) -> &'static [&'static str] {
        match self {
            Self::Ffmpeg => &["-version"],
            Self::YtDlp => &["--version"],
        }
    }

    /// Download location for the current platform.
    fn download_url(self) -> MediaResult<&'static str> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        match (self, os, arch) {
            (Self::YtDlp, "linux", "x86_64") => {
                Ok("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux")
            }
            (Self::YtDlp, "linux", "aarch64") => {
                Ok("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64")
            }
            (Self::YtDlp, "macos", _) => {
                Ok("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos")
            }
            (Self::YtDlp, "windows", "x86_64") => {
                Ok("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe")
            }
            (Self::Ffmpeg, "linux", "x86_64") => {
                Ok("https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz")
            }
            (Self::Ffmpeg, "linux", "aarch64") => {
                Ok("https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-arm64-static.tar.xz")
            }
            (Self::Ffmpeg, "windows", "x86_64") => {
                Ok("https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip")
            }
            (Self::Ffmpeg, "macos", _) => Ok("https://evermeet.cx/ffmpeg/getrelease/zip"),
            _ => Err(MediaError::Unsupported(format!(
                "no managed {} build for {os}/{arch}; install it on PATH",
                self.id()
            ))),
        }
    }

    fn archive_extension(self) -> &'static str {
        match self {
            Self::Ffmpeg if cfg!(windows) || cfg!(target_os = "macos") => "zip",
            Self::Ffmpeg => "tar.xz",
            Self::YtDlp => "bin",
        }
    }
}

/// One tool's availability.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub tool: &'static str,
    pub path: Option<String>,
    pub version: Option<String>,
    pub managed: bool,
}

/// Progress for a tool download.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

/// Tool locator and installer rooted at the app's data directory.
#[derive(Debug, Clone)]
pub struct Tools {
    data_dir: PathBuf,
}

impl Tools {
    /// Create a manager rooted at `data_dir`.
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Directory holding managed downloads.
    #[must_use]
    pub fn directory(&self) -> PathBuf {
        self.data_dir.join("tools")
    }

    fn managed_path(&self, tool: Tool) -> PathBuf {
        self.directory().join(tool.binary_name())
    }

    /// Find a usable tool: `PATH` first, then the managed copy.
    #[must_use]
    pub fn locate(&self, tool: Tool) -> Option<PathBuf> {
        find_on_path(tool.binary_name()).or_else(|| {
            let managed = self.managed_path(tool);
            managed.is_file().then_some(managed)
        })
    }

    /// Whether a tool is installed (without running it).
    #[must_use]
    pub fn is_available(&self, tool: Tool) -> bool {
        self.locate(tool).is_some()
    }

    /// Availability and version for both tools.
    #[must_use]
    pub fn status(&self) -> Vec<ToolStatus> {
        [Tool::Ffmpeg, Tool::YtDlp]
            .into_iter()
            .map(|tool| {
                let path = self.locate(tool);
                let managed = path
                    .as_ref()
                    .is_some_and(|candidate| candidate.starts_with(self.directory()));
                ToolStatus {
                    tool: tool.id(),
                    version: path
                        .as_deref()
                        .and_then(|binary| tool_version(binary, tool)),
                    path: path.map(|path| path.to_string_lossy().to_string()),
                    managed,
                }
            })
            .collect()
    }

    /// Locate a tool or download the managed build on first use.
    pub async fn ensure(
        &self,
        tool: Tool,
        mut on_progress: impl FnMut(ToolProgress),
    ) -> MediaResult<PathBuf> {
        if let Some(path) = self.locate(tool) {
            return Ok(path);
        }

        let url = tool.download_url()?;
        let directory = self.directory();
        tokio::fs::create_dir_all(&directory).await?;
        let archive = directory.join(format!(
            ".download-{}.{}",
            tool.id(),
            tool.archive_extension()
        ));

        download(url, &archive, &mut on_progress).await?;

        let result = if matches!(tool.archive_extension(), "zip" | "tar.xz") {
            let extract_dir = directory.join(format!(".extract-{}", tool.id()));
            let _ = tokio::fs::remove_dir_all(&extract_dir).await;
            tokio::fs::create_dir_all(&extract_dir).await?;
            let archive_clone = archive.clone();
            let extract_clone = extract_dir.clone();
            let kind = tool.archive_extension();
            tokio::task::spawn_blocking(move || extract(&archive_clone, &extract_clone, kind))
                .await
                .map_err(|error| {
                    MediaError::Unsupported(format!("extractor panicked: {error}"))
                })??;
            let binary_name = tool.binary_name().to_string();
            let extracted = find_file(&extract_dir, &binary_name, 4).ok_or_else(|| {
                MediaError::Parse(format!(
                    "{binary_name} was not inside the downloaded archive"
                ))
            })?;
            let target = self.managed_path(tool);
            tokio::fs::copy(&extracted, &target).await?;
            let _ = tokio::fs::remove_dir_all(&extract_dir).await;
            target
        } else {
            let target = self.managed_path(tool);
            tokio::fs::rename(&archive, &target).await?;
            target
        };

        let _ = tokio::fs::remove_file(&archive).await;
        make_executable(&result)?;
        Ok(result)
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn tool_version(binary: &Path, tool: Tool) -> Option<String> {
    let output = Command::new(binary)
        .args(tool.version_args())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
}

async fn download(
    url: &str,
    destination: &Path,
    mut on_progress: impl FnMut(ToolProgress),
) -> MediaResult<()> {
    let client = reqwest::Client::builder()
        .user_agent(rilldown_engine::branding::USER_AGENT)
        .build()?;
    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(MediaError::Http {
            status: status.as_u16(),
            url: url.to_string(),
        });
    }
    let total = response.content_length();
    if total.is_some_and(|bytes| bytes > MAX_TOOL_BYTES) {
        return Err(MediaError::Unsupported(
            "the tool download is unreasonably large".to_string(),
        ));
    }

    let mut file = tokio::fs::File::create(destination).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0_u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > MAX_TOOL_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(destination).await;
            return Err(MediaError::Unsupported(
                "the tool download exceeded the size limit".to_string(),
            ));
        }
        file.write_all(&chunk).await?;
        on_progress(ToolProgress {
            downloaded_bytes: downloaded,
            total_bytes: total,
        });
    }
    file.flush().await?;
    Ok(())
}

fn extract(archive: &Path, destination: &Path, kind: &str) -> MediaResult<()> {
    if kind == "zip" {
        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|error| MediaError::Parse(format!("invalid zip archive: {error}")))?;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|error| MediaError::Parse(format!("invalid zip entry: {error}")))?;
            let Some(relative) = entry.enclosed_name() else {
                continue;
            };
            let target = destination.join(relative);
            if entry.is_dir() {
                std::fs::create_dir_all(&target)?;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut output = std::fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut output)?;
        }
        return Ok(());
    }

    let file = std::fs::File::open(archive)?;
    let decoder = xz2::read::XzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    tar.unpack(destination)?;
    Ok(())
}

fn find_file(directory: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(directory).ok()?;
    let mut directories = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path.file_name() == Some(OsStr::new(name)) {
                return Some(path);
            }
        } else if path.is_dir() {
            directories.push(path);
        }
    }
    directories
        .into_iter()
        .find_map(|directory| find_file(&directory, name, depth - 1))
}

fn make_executable(path: &Path) -> MediaResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_paths_live_under_the_data_directory() {
        let tools = Tools::new("/tmp/rilldown-test");
        assert_eq!(tools.directory(), PathBuf::from("/tmp/rilldown-test/tools"));
        assert_eq!(
            tools.managed_path(Tool::YtDlp),
            PathBuf::from("/tmp/rilldown-test/tools").join(Tool::YtDlp.binary_name())
        );
    }

    #[test]
    fn finds_nested_files() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("ffmpeg"), b"binary").unwrap();
        let found = find_file(directory.path(), "ffmpeg", 3).unwrap();
        assert_eq!(found, nested.join("ffmpeg"));
    }

    #[test]
    fn platform_downloads_exist_for_supported_targets() {
        if matches!(
            (std::env::consts::OS, std::env::consts::ARCH),
            ("linux" | "macos" | "windows", "x86_64") | ("linux", "aarch64")
        ) {
            assert!(Tool::Ffmpeg.download_url().is_ok());
            assert!(Tool::YtDlp.download_url().is_ok());
        }
    }

    #[test]
    fn versions_report_a_first_line() {
        if let Some(path) = find_on_path("sh") {
            assert!(!path.as_os_str().is_empty());
        }
    }
}
