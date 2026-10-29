//! Native-messaging host registration.
//!
//! Browsers only launch a native host that has a manifest in a
//! browser-specific directory (or registry key on Windows). This module writes
//! those manifests for the browsers it can find and removes them again.
//!
//! Chrome does not support arguments in native-messaging manifests, so the host
//! is the separate `rilldown-native-host` executable that ships beside the app.

use std::path::{Path, PathBuf};

use serde::Serialize;

use rilldown_engine::branding;

/// Native host name, shared by every manifest and registry key.
pub const HOST_NAME: &str = branding::APP_ID;
/// Stable Chrome extension id derived from the public key in the manifest.
pub const CHROME_EXTENSION_ID: &str = "bkcbdfegbgmhalklgbhjgdefopbglmif";
/// Stable Firefox add-on id.
pub const FIREFOX_EXTENSION_ID: &str = "rilldown@rilldown.com";

const HOST_BINARY: &str = if cfg!(windows) {
    "rilldown-native-host.exe"
} else {
    "rilldown-native-host"
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostStatus {
    pub installed: bool,
    pub executable: Option<String>,
    pub chrome_extension_id: &'static str,
    pub firefox_extension_id: &'static str,
    pub manifests: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostInstall {
    pub executable: String,
    pub chrome_extension_id: &'static str,
    pub firefox_extension_id: &'static str,
    pub manifests: Vec<String>,
    pub browsers: Vec<String>,
}

/// Locate the native host executable that ships with the app.
#[must_use]
pub fn host_executable() -> Option<PathBuf> {
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            let candidate = directory.join(HOST_BINARY);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(HOST_BINARY))
            .find(|candidate| candidate.is_file())
    })
}

/// Chrome-family browser profile directories and their host manifest folders.
fn chromium_roots() -> Vec<(&'static str, PathBuf, PathBuf)> {
    let mut roots = Vec::new();
    if cfg!(target_os = "windows") {
        return roots;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                if cfg!(target_os = "macos") {
                    PathBuf::from(home)
                        .join("Library")
                        .join("Application Support")
                } else {
                    PathBuf::from(home).join(".config")
                }
            })
        });
    let Some(config) = config else {
        return roots;
    };

    let browsers = if cfg!(target_os = "macos") {
        vec![
            ("Google Chrome", config.join("Google/Chrome")),
            ("Chromium", config.join("Chromium")),
            ("Brave", config.join("BraveSoftware/Brave-Browser")),
            ("Microsoft Edge", config.join("Microsoft Edge")),
        ]
    } else {
        vec![
            ("Google Chrome", config.join("google-chrome")),
            ("Chromium", config.join("chromium")),
            ("Brave", config.join("BraveSoftware/Brave-Browser")),
            ("Microsoft Edge", config.join("microsoft-edge")),
        ]
    };

    roots.extend(browsers.into_iter().map(|(name, base)| {
        let manifest = base.join("NativeMessagingHosts");
        (name, base, manifest)
    }));
    roots
}

/// Firefox's native messaging directory.
fn firefox_root() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        return std::env::var_os("APPDATA")
            .map(|appdata| PathBuf::from(appdata).join("Mozilla/NativeMessagingHosts"));
    }
    std::env::var_os("HOME").map(|home| {
        if cfg!(target_os = "macos") {
            PathBuf::from(home).join("Library/Application Support/Mozilla/NativeMessagingHosts")
        } else {
            PathBuf::from(home).join(".mozilla/native-messaging-hosts")
        }
    })
}

fn manifest_json(executable: &Path, chromium: bool) -> serde_json::Value {
    if chromium {
        serde_json::json!({
            "name": HOST_NAME,
            "description": "Rilldown download bridge",
            "path": executable.to_string_lossy(),
            "type": "stdio",
            "allowed_origins": [format!("chrome-extension://{CHROME_EXTENSION_ID}/")],
        })
    } else {
        serde_json::json!({
            "name": HOST_NAME,
            "description": "Rilldown download bridge",
            "path": executable.to_string_lossy(),
            "type": "stdio",
            "allowed_extensions": [FIREFOX_EXTENSION_ID],
        })
    }
}

fn write_manifest(path: &Path, executable: &Path, chromium: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let encoded = serde_json::to_vec_pretty(&manifest_json(executable, chromium))
        .map_err(|error| error.to_string())?;
    std::fs::write(path, encoded)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Which manifests currently exist for this user.
#[must_use]
pub fn status() -> BrowserHostStatus {
    let mut manifests = Vec::new();
    for (_, _, root) in chromium_roots() {
        let path = root.join(format!("{HOST_NAME}.json"));
        if path.is_file() {
            manifests.push(path.to_string_lossy().to_string());
        }
    }
    if let Some(root) = firefox_root() {
        let path = root.join(format!("{HOST_NAME}.json"));
        if path.is_file() {
            manifests.push(path.to_string_lossy().to_string());
        }
    }
    #[cfg(windows)]
    {
        manifests.extend(windows_manifests());
    }
    BrowserHostStatus {
        installed: !manifests.is_empty(),
        executable: host_executable().map(|path| path.to_string_lossy().to_string()),
        chrome_extension_id: CHROME_EXTENSION_ID,
        firefox_extension_id: FIREFOX_EXTENSION_ID,
        manifests,
    }
}

/// Install manifests for every browser directory this user has.
pub fn install() -> Result<BrowserHostInstall, String> {
    let executable =
        host_executable().ok_or_else(|| format!("{HOST_BINARY} is missing beside the app"))?;
    let mut manifests = Vec::new();
    let mut browsers = Vec::new();

    for (browser, base, root) in chromium_roots() {
        // Only register browsers that already have a profile directory;
        // writing into a Chrome directory for a user who never ran Chrome just
        // leaves junk behind.
        if !base.is_dir() {
            continue;
        }
        let path = root.join(format!("{HOST_NAME}.json"));
        write_manifest(&path, &executable, true)?;
        manifests.push(path.to_string_lossy().to_string());
        browsers.push(browser.to_string());
    }

    if let Some(root) = firefox_root() {
        let path = root.join(format!("{HOST_NAME}.json"));
        write_manifest(&path, &executable, false)?;
        manifests.push(path.to_string_lossy().to_string());
        browsers.push("Firefox".to_string());
    }

    #[cfg(windows)]
    {
        let manifest = windows_manifest_path();
        write_manifest(&manifest, &executable, true)?;
        windows_register(&manifest)?;
        manifests.push(manifest.to_string_lossy().to_string());
        browsers.extend(
            ["Chrome", "Chromium", "Brave", "Edge"]
                .iter()
                .map(|name| (*name).to_string()),
        );
    }

    Ok(BrowserHostInstall {
        executable: executable.to_string_lossy().to_string(),
        chrome_extension_id: CHROME_EXTENSION_ID,
        firefox_extension_id: FIREFOX_EXTENSION_ID,
        manifests,
        browsers,
    })
}

/// Remove every manifest this user has.
pub fn uninstall() -> usize {
    let mut removed = 0;
    for (_, _, root) in chromium_roots() {
        let path = root.join(format!("{HOST_NAME}.json"));
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    if let Some(root) = firefox_root() {
        let path = root.join(format!("{HOST_NAME}.json"));
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    #[cfg(windows)]
    {
        removed += windows_unregister();
        let manifest = windows_manifest_path();
        if std::fs::remove_file(manifest).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(windows)]
fn data_dir() -> PathBuf {
    rilldown_engine::EngineConfig::platform_default()
        .map_or_else(|_| PathBuf::from("."), |config| config.data_dir)
}

#[cfg(windows)]
fn windows_manifest_path() -> PathBuf {
    data_dir().join(format!("{HOST_NAME}.json"))
}

#[cfg(windows)]
fn windows_manifest_roots() -> [(&'static str, &'static str); 4] {
    [
        (
            "Google Chrome",
            r"Software\Google\Chrome\NativeMessagingHosts",
        ),
        ("Chromium", r"Software\Chromium\NativeMessagingHosts"),
        (
            "Brave",
            r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
        ),
        (
            "Microsoft Edge",
            r"Software\Microsoft\Edge\NativeMessagingHosts",
        ),
    ]
}

#[cfg(windows)]
fn windows_register(manifest: &Path) -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for (_, base) in windows_manifest_roots() {
        let key = hkcu
            .create_subkey(format!(r"{base}\{HOST_NAME}"))
            .map_err(|error| format!("cannot register native host: {error}"))?;
        key.0
            .set_value("", &manifest.to_string_lossy().to_string())
            .map_err(|error| format!("cannot write native host registry value: {error}"))?;
    }
    Ok(())
}

#[cfg(windows)]
fn windows_unregister() -> usize {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut removed = 0;
    for (_, base) in windows_manifest_roots() {
        if hkcu
            .delete_subkey_all(format!(r"{base}\{HOST_NAME}"))
            .is_ok()
        {
            removed += 1;
        }
    }
    removed
}

#[cfg(windows)]
fn windows_manifests() -> Vec<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let mut manifests = Vec::new();
    for (_, base) in windows_manifest_roots() {
        if let Ok(key) = hkcu.open_subkey(format!(r"{base}\{HOST_NAME}")) {
            if let Ok(value) = key.get_value::<String, _>("") {
                manifests.push(value);
            }
        }
    }
    manifests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_manifest_shape_is_valid() {
        let manifest = manifest_json(Path::new("/opt/rilldown-native-host"), true);
        assert_eq!(manifest["name"], HOST_NAME);
        assert_eq!(manifest["type"], "stdio");
        assert_eq!(
            manifest["allowed_origins"][0],
            format!("chrome-extension://{CHROME_EXTENSION_ID}/")
        );
        assert!(manifest.get("args").is_none());
    }

    #[test]
    fn firefox_manifest_uses_an_addon_id() {
        let manifest = manifest_json(Path::new("/opt/rilldown-native-host"), false);
        assert_eq!(manifest["allowed_extensions"][0], FIREFOX_EXTENSION_ID);
        assert!(manifest.get("allowed_origins").is_none());
    }

    #[test]
    fn writes_a_chromium_manifest_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join("NativeMessagingHosts")
            .join("host.json");
        write_manifest(&path, Path::new("/opt/rilldown-native-host"), true).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["name"], HOST_NAME);
        assert_eq!(parsed["path"], "/opt/rilldown-native-host");
        assert_eq!(
            parsed["allowed_origins"][0],
            format!("chrome-extension://{CHROME_EXTENSION_ID}/")
        );
    }
}
