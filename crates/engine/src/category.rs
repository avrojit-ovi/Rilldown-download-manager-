//! Automatic download categories.
//!
//! Categories are derived from the probe result, never from user input, so a
//! server cannot choose an arbitrary folder. The shell maps each category to a
//! subdirectory of the configured download directory.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};

/// Broad file-type bucket used by the sidebar and save paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Compressed,
    Documents,
    Music,
    Video,
    Programs,
    Other,
}

impl Category {
    /// Every category, in display order.
    pub const ALL: [Self; 6] = [
        Self::Compressed,
        Self::Documents,
        Self::Music,
        Self::Video,
        Self::Programs,
        Self::Other,
    ];

    /// Stable identifier used in the database and protocol.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Compressed => "compressed",
            Self::Documents => "documents",
            Self::Music => "music",
            Self::Video => "video",
            Self::Programs => "programs",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.id())
    }
}

impl FromStr for Category {
    type Err = EngineError;

    fn from_str(value: &str) -> EngineResult<Self> {
        match value {
            "compressed" => Ok(Self::Compressed),
            "documents" => Ok(Self::Documents),
            "music" => Ok(Self::Music),
            "video" => Ok(Self::Video),
            "programs" => Ok(Self::Programs),
            "other" => Ok(Self::Other),
            other => Err(EngineError::Other(format!(
                "unknown download category: {other}"
            ))),
        }
    }
}

/// Classify a filename, falling back to the declared content type.
#[must_use]
pub fn classify(filename: &str, content_type: Option<&str>) -> Category {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "lz4" | "cab"
        | "iso" => Category::Compressed,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "epub"
        | "md" | "csv" | "odt" | "ods" | "odp" => Category::Documents,
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "oga" | "m4a" | "opus" | "wma" | "aiff" => {
            Category::Music
        }
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "mpg" | "mpeg" | "ts" | "flv" | "wmv" => {
            Category::Video
        }
        "exe" | "msi" | "msix" | "pkg" | "dmg" | "deb" | "rpm" | "appimage" | "apk" | "app"
        | "snap" | "flatpak" | "jar" => Category::Programs,
        _ => classify_content_type(content_type),
    }
}

fn classify_content_type(content_type: Option<&str>) -> Category {
    let essence = content_type
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if essence.starts_with("video/") {
        Category::Video
    } else if essence.starts_with("audio/") {
        Category::Music
    } else if essence.starts_with("text/")
        || essence == "application/pdf"
        || essence == "application/epub+zip"
        || essence.starts_with("application/vnd.openxmlformats")
        || essence.starts_with("application/vnd.oasis")
        || essence == "application/msword"
    {
        Category::Documents
    } else if essence.starts_with("application/zip")
        || essence.starts_with("application/x-tar")
        || essence.starts_with("application/gzip")
        || essence.starts_with("application/x-7z")
    {
        Category::Compressed
    } else {
        Category::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension() {
        assert_eq!(classify("archive.tar.gz", None), Category::Compressed);
        assert_eq!(classify("manual.PDF", None), Category::Documents);
        assert_eq!(classify("song.flac", None), Category::Music);
        assert_eq!(classify("movie.mkv", None), Category::Video);
        assert_eq!(classify("setup.AppImage", None), Category::Programs);
        assert_eq!(classify("unknown.bin", None), Category::Other);
    }

    #[test]
    fn falls_back_to_content_type() {
        assert_eq!(
            classify("download", Some("video/mp4; charset=binary")),
            Category::Video
        );
        assert_eq!(classify("download", Some("audio/mpeg")), Category::Music);
        assert_eq!(
            classify("download", Some("application/pdf")),
            Category::Documents
        );
        assert_eq!(
            classify("download", Some("application/zip")),
            Category::Compressed
        );
        assert_eq!(classify("download", None), Category::Other);
    }

    #[test]
    fn identifiers_round_trip() {
        for category in Category::ALL {
            assert_eq!(category.id().parse::<Category>().unwrap(), category);
        }
        assert!("nonsense".parse::<Category>().is_err());
    }
}
