//! Rilldown desktop shell.
//!
//! The shell owns windowing, platform integration, and the UI command surface.
//! Download behavior lives in `rilldown-engine`; media extraction lives in
//! `rilldown-media`. Commands here are thin adapters so both crates stay
//! testable without a `WebView`.

// Tauri commands must take owned arguments because they are deserialized from
// the IPC payload; taking references is not supported by the macro.
#![allow(clippy::needless_pass_by_value)]

mod bridge;
mod browser_host;
mod settings;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rilldown_engine::branding;
use rilldown_engine::{
    Checksum, DEFAULT_QUEUE_ID, DownloadFilter, DownloadId, DownloadRecord, DownloadSpec, Engine,
    EngineConfig, Queue, Url,
};
use rilldown_media::{GrabEvent, GrabRequest, MediaInfo, Tool, ToolProgress, ToolStatus, Tools};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::settings::AppSettings;

/// Identity of the running application.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    name: &'static str,
    version: &'static str,
    identifier: &'static str,
    engine_version: &'static str,
    platform: &'static str,
    arch: &'static str,
}

/// Identity of the embedded engine.
#[derive(Debug, serde::Serialize)]
struct EngineInfo {
    name: &'static str,
    version: &'static str,
}

/// Shared shell state.
#[derive(Debug)]
pub struct AppState {
    pub(crate) engine: Engine,
    pub(crate) settings: Mutex<AppSettings>,
    pub(crate) tools: Tools,
    pub(crate) media_jobs: Mutex<HashMap<String, MediaJob>>,
    pub(crate) media_cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl AppState {
    fn new(engine: Engine, settings: AppSettings) -> Self {
        let tools = Tools::new(engine.config().data_dir.clone());
        Self {
            engine,
            settings: Mutex::new(settings),
            tools,
            media_jobs: Mutex::new(HashMap::new()),
            media_cancels: Mutex::new(HashMap::new()),
        }
    }
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: branding::APP_NAME,
        version: branding::APP_VERSION,
        identifier: branding::APP_ID,
        engine_version: branding::APP_VERSION,
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[tauri::command]
fn engine_info() -> EngineInfo {
    EngineInfo {
        name: branding::APP_NAME,
        version: branding::APP_VERSION,
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings.lock().expect("settings lock").clone()
}

#[tauri::command]
fn save_settings(
    state: State<'_, AppState>,
    mut settings: AppSettings,
) -> Result<AppSettings, String> {
    settings.validate();
    std::fs::create_dir_all(&settings.download_dir)
        .map_err(|error| format!("cannot create {}: {error}", settings.download_dir.display()))?;
    settings
        .save(&state.engine)
        .map_err(|error| error.to_string())?;
    settings.apply(&state.engine);
    *state.settings.lock().expect("settings lock") = settings.clone();
    Ok(settings)
}

#[tauri::command]
async fn choose_directory(app: AppHandle) -> Option<String> {
    let folder =
        tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
            .await
            .ok()
            .flatten()?;
    Some(folder.into_path().ok()?.to_string_lossy().to_string())
}

#[tauri::command]
fn list_downloads(state: State<'_, AppState>) -> Result<Vec<DownloadRecord>, String> {
    state
        .engine
        .list(&DownloadFilter::default())
        .map_err(|error| error.to_string())
}

/// Input accepted from the UI, the CLI-compatible bridge, and batch pastes.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddInput {
    url: String,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    save_dir: Option<PathBuf>,
    #[serde(default)]
    connections: Option<u8>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    cookies: Option<String>,
    #[serde(default)]
    referrer: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default)]
    speed_limit_bps: Option<u64>,
    #[serde(default)]
    checksum: Option<Checksum>,
    #[serde(default)]
    queue_id: Option<String>,
}

fn build_spec(input: AddInput, settings: &AppSettings) -> Result<DownloadSpec, String> {
    let url = Url::parse(input.url.trim()).map_err(|error| format!("invalid URL: {error}"))?;
    let save_dir = input
        .save_dir
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| settings.download_dir.clone());
    let mut spec = DownloadSpec::new(url, save_dir);
    spec.filename = input.filename.filter(|name| !name.trim().is_empty());
    spec.connections = input.connections.unwrap_or(settings.connections);
    spec.headers = input.headers;
    spec.cookies = input.cookies.filter(|value| !value.trim().is_empty());
    spec.referrer = input.referrer;
    spec.user_agent = input
        .user_agent
        .filter(|value| !value.trim().is_empty())
        .or_else(|| settings.user_agent.clone());
    spec.proxy = input
        .proxy
        .filter(|value| !value.trim().is_empty())
        .or_else(|| settings.proxy.clone());
    // The settings speed limit is engine-wide; a per-download limit only comes
    // from the add dialog. Applying both would double-throttle the transfer.
    spec.speed_limit_bps = input.speed_limit_bps;
    spec.checksum = input.checksum;
    spec.queue_id = Some(
        input
            .queue_id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_QUEUE_ID.to_string()),
    );
    Ok(spec)
}

#[tauri::command]
async fn add_download(
    state: State<'_, AppState>,
    input: AddInput,
) -> Result<DownloadRecord, String> {
    let settings = state.settings.lock().expect("settings lock").clone();
    let spec = build_spec(input, &settings)?;
    state
        .engine
        .add(spec)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchOutcome {
    url: String,
    ok: bool,
    record: Option<DownloadRecord>,
    error: Option<String>,
}

#[tauri::command]
async fn add_downloads(
    state: State<'_, AppState>,
    inputs: Vec<AddInput>,
) -> Result<Vec<BatchOutcome>, String> {
    let settings = state.settings.lock().expect("settings lock").clone();
    let mut outcomes = Vec::with_capacity(inputs.len());
    for input in inputs {
        let url = input.url.clone();
        match build_spec(input, &settings) {
            Ok(spec) => match state.engine.add(spec).await {
                Ok(record) => outcomes.push(BatchOutcome {
                    url,
                    ok: true,
                    record: Some(record),
                    error: None,
                }),
                Err(error) => outcomes.push(BatchOutcome {
                    url,
                    ok: false,
                    record: None,
                    error: Some(error.to_string()),
                }),
            },
            Err(error) => outcomes.push(BatchOutcome {
                url,
                ok: false,
                record: None,
                error: Some(error),
            }),
        }
    }
    Ok(outcomes)
}

fn parse_id(id: &str) -> Result<DownloadId, String> {
    id.parse::<DownloadId>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn pause_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .engine
        .pause(parse_id(&id)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn resume_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .engine
        .resume(parse_id(&id)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn cancel_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .engine
        .cancel(parse_id(&id)?)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn remove_download(
    state: State<'_, AppState>,
    id: String,
    delete_file: bool,
) -> Result<(), String> {
    state
        .engine
        .remove(parse_id(&id)?, delete_file)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pause_all(state: State<'_, AppState>) -> Result<usize, String> {
    state
        .engine
        .pause_all()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn resume_all(state: State<'_, AppState>) -> Result<usize, String> {
    state
        .engine
        .resume_all()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_download(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let record = state
        .engine
        .get(parse_id(&id)?)
        .map_err(|error| error.to_string())?;
    let path = record.destination();
    if !path.is_file() {
        return Err("the file is no longer on disk".to_string());
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reveal_download(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let record = state
        .engine
        .get(parse_id(&id)?)
        .map_err(|error| error.to_string())?;
    let path = record.destination();
    let target = if path.exists() { path } else { record.save_dir };
    app.opener()
        .reveal_item_in_dir(target)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_downloads_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let directory = state
        .settings
        .lock()
        .expect("settings lock")
        .download_dir
        .clone();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    app.opener()
        .open_path(directory.to_string_lossy(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_queues(state: State<'_, AppState>) -> Result<Vec<Queue>, String> {
    state.engine.queues().map_err(|error| error.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueInput {
    id: String,
    name: String,
    #[serde(default)]
    max_concurrent: u32,
    #[serde(default)]
    speed_limit_bps: Option<u64>,
}

#[tauri::command]
fn save_queue(state: State<'_, AppState>, queue: QueueInput) -> Result<Vec<Queue>, String> {
    let existing = state
        .engine
        .queues()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|candidate| candidate.id == queue.id);
    let saved = Queue {
        id: queue.id,
        name: queue.name,
        max_concurrent: queue.max_concurrent,
        speed_limit_bps: queue.speed_limit_bps,
        created_at: existing.map_or_else(chrono::Utc::now, |queue| queue.created_at),
    };
    state
        .engine
        .save_queue(saved)
        .map_err(|error| error.to_string())?;
    state.engine.queues().map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_queue(state: State<'_, AppState>, id: String) -> Result<Vec<Queue>, String> {
    state
        .engine
        .remove_queue(&id)
        .map_err(|error| error.to_string())?;
    state.engine.queues().map_err(|error| error.to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    available: bool,
    version: Option<String>,
    notes: Option<String>,
    date: Option<String>,
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|error| error.to_string())?;
    let update = updater.check().await.map_err(|error| error.to_string())?;
    Ok(match update {
        Some(update) => UpdateInfo {
            available: true,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            date: update.date.map(|date| date.to_string()),
        },
        None => UpdateInfo {
            available: false,
            version: None,
            notes: None,
            date: None,
        },
    })
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|error| error.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Rilldown is already up to date".to_string())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    app.restart();
}

#[tauri::command]
fn browser_host_status() -> browser_host::BrowserHostStatus {
    browser_host::status()
}

#[tauri::command]
fn install_browser_host() -> Result<browser_host::BrowserHostInstall, String> {
    browser_host::install()
}

#[tauri::command]
fn uninstall_browser_host() -> usize {
    browser_host::uninstall()
}

#[tauri::command]
fn open_extension_folder(app: AppHandle) -> Result<String, String> {
    let directory = extension_directory(&app)
        .ok_or_else(|| "the browser extension folder was not found".to_string())?;
    app.opener()
        .open_path(directory.to_string_lossy(), None::<&str>)
        .map_err(|error| error.to_string())?;
    Ok(directory.to_string_lossy().to_string())
}

fn extension_directory(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(resources) = app.path().resource_dir() {
        let candidate = resources.join("browser-extension/chrome");
        if candidate.join("manifest.json").is_file() {
            return Some(candidate);
        }
    }
    let mut cursor = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for _ in 0..5 {
        let candidate = cursor.join("extension/dist/chrome");
        if candidate.join("manifest.json").is_file() {
            return Some(candidate);
        }
        cursor = cursor.parent()?.to_path_buf();
    }
    None
}

#[tauri::command]
fn tools_status(state: State<'_, AppState>) -> Vec<ToolStatus> {
    state.tools.status()
}

fn parse_tool(tool: &str) -> Result<Tool, String> {
    match tool {
        "ffmpeg" => Ok(Tool::Ffmpeg),
        "yt-dlp" | "ytdlp" => Ok(Tool::YtDlp),
        other => Err(format!("unknown tool: {other}")),
    }
}

#[tauri::command]
async fn install_tool(
    app: AppHandle,
    state: State<'_, AppState>,
    tool: String,
) -> Result<Vec<ToolStatus>, String> {
    let tool = parse_tool(&tool)?;
    let tools = state.tools.clone();
    let emitter = app.clone();
    tools
        .ensure(tool, move |progress: ToolProgress| {
            let _ = emitter.emit("tools://progress", progress);
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(tools.status())
}

#[tauri::command]
async fn media_probe(state: State<'_, AppState>, url: String) -> Result<MediaInfo, String> {
    let parsed = Url::parse(url.trim()).map_err(|error| format!("invalid URL: {error}"))?;
    let tools = state.tools.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    rilldown_media::probe(&parsed, &tools, &cancel)
        .await
        .map_err(|error| error.to_string())
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MediaJob {
    id: String,
    url: String,
    title: String,
    format_id: Option<String>,
    state: String,
    percent: Option<f64>,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    speed_bps: u64,
    eta_seconds: Option<u64>,
    stage: String,
    error: Option<String>,
    path: Option<String>,
    created_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaStartInput {
    url: String,
    #[serde(default)]
    format_id: Option<String>,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    referrer: Option<String>,
}

#[tauri::command]
async fn media_start(
    app: AppHandle,
    state: State<'_, AppState>,
    input: MediaStartInput,
) -> Result<MediaJob, String> {
    let url = Url::parse(input.url.trim()).map_err(|error| format!("invalid URL: {error}"))?;
    if !state.tools.is_available(Tool::YtDlp) {
        return Err("yt-dlp is not installed yet".to_string());
    }
    let output_dir = state
        .settings
        .lock()
        .expect("settings lock")
        .download_dir
        .clone();
    let job = MediaJob {
        id: uuid::Uuid::new_v4().to_string(),
        url: url.to_string(),
        title: url.to_string(),
        format_id: input.format_id.clone(),
        state: "queued".to_string(),
        percent: None,
        downloaded_bytes: 0,
        total_bytes: None,
        speed_bps: 0,
        eta_seconds: None,
        stage: "starting".to_string(),
        error: None,
        path: None,
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    state
        .media_jobs
        .lock()
        .expect("media jobs lock")
        .insert(job.id.clone(), job.clone());
    let _ = app.emit("media://job", &job);

    let tools = state.tools.clone();
    let request = GrabRequest {
        url,
        format_id: input.format_id,
        output_dir,
        filename: input.filename,
        referrer: input.referrer,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .media_cancels
        .lock()
        .expect("media cancels lock")
        .insert(job.id.clone(), cancel.clone());

    let job_id = job.id.clone();
    let task_app = app.clone();
    tauri::async_runtime::spawn(async move {
        update_media_job(&task_app, &job_id, |job| {
            job.state = "downloading".to_string();
            job.stage = "starting".to_string();
        });
        let event_app = task_app.clone();
        let event_job = job_id.clone();
        let result = rilldown_media::grab(&request, &tools, cancel, move |event| {
            apply_grab_event(&event_app, &event_job, event);
        })
        .await;

        match result {
            Ok(outcome) => {
                let path = outcome.path.to_string_lossy().to_string();
                let title = outcome.title.clone();
                update_media_job(&task_app, &job_id, |job| {
                    job.state = "completed".to_string();
                    job.percent = Some(100.0);
                    job.path = Some(path.clone());
                    job.title.clone_from(&title);
                    job.stage = "completed".to_string();
                    job.error = None;
                });
                notify(&task_app, "Media ready", &title);
            }
            Err(rilldown_media::MediaError::Cancelled) => {
                update_media_job(&task_app, &job_id, |job| {
                    job.state = "cancelled".to_string();
                    job.stage = "cancelled".to_string();
                });
            }
            Err(error) => {
                let message = error.to_string();
                update_media_job(&task_app, &job_id, |job| {
                    job.state = "failed".to_string();
                    job.error = Some(message.clone());
                    job.stage = "failed".to_string();
                });
            }
        }

        if let Some(state) = task_app.try_state::<AppState>() {
            state
                .media_cancels
                .lock()
                .expect("media cancels lock")
                .remove(&job_id);
        }
    });

    Ok(job)
}

fn update_media_job(app: &AppHandle, id: &str, update: impl FnOnce(&mut MediaJob)) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut jobs = state.media_jobs.lock().expect("media jobs lock");
    let Some(job) = jobs.get_mut(id) else {
        return;
    };
    update(job);
    let snapshot = job.clone();
    drop(jobs);
    let _ = app.emit("media://job", &snapshot);
}

fn apply_grab_event(app: &AppHandle, id: &str, event: GrabEvent) {
    match event {
        GrabEvent::Progress {
            percent,
            downloaded_bytes,
            total_bytes,
            speed_bps,
            eta_seconds,
        } => update_media_job(app, id, |job| {
            job.percent = percent;
            if let Some(bytes) = downloaded_bytes {
                job.downloaded_bytes = bytes;
            }
            job.total_bytes = total_bytes;
            if let Some(speed) = speed_bps {
                job.speed_bps = speed;
            }
            job.eta_seconds = eta_seconds;
            job.stage = "downloading".to_string();
        }),
        GrabEvent::Stage { stage } => update_media_job(app, id, |job| {
            job.stage = stage;
        }),
        GrabEvent::Log { message } => {
            tracing::debug!(job = id, %message, "media grab log");
        }
    }
}

#[tauri::command]
fn media_jobs(state: State<'_, AppState>) -> Vec<MediaJob> {
    let mut jobs: Vec<MediaJob> = state
        .media_jobs
        .lock()
        .expect("media jobs lock")
        .values()
        .cloned()
        .collect();
    jobs.sort_by_key(|job| std::cmp::Reverse(job.created_at));
    jobs
}

#[tauri::command]
fn media_cancel(state: State<'_, AppState>, id: String) -> bool {
    state
        .media_cancels
        .lock()
        .expect("media cancels lock")
        .get(&id)
        .is_some_and(|flag| {
            flag.store(true, Ordering::SeqCst);
            true
        })
}

#[tauri::command]
fn media_remove(state: State<'_, AppState>, id: String) {
    state
        .media_jobs
        .lock()
        .expect("media jobs lock")
        .remove(&id);
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let enabled = app
        .try_state::<AppState>()
        .is_some_and(|state| state.settings.lock().expect("settings lock").notifications);
    if !enabled {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

fn engine_config() -> EngineConfig {
    let base = EngineConfig::platform_default()
        .unwrap_or_else(|_| EngineConfig::new("./data", "./downloads"));
    let data_dir = std::env::var_os("RILLDOWN_DATA_DIR").map_or(base.data_dir, PathBuf::from);
    let download_dir =
        std::env::var_os("RILLDOWN_DOWNLOAD_DIR").map_or(base.download_dir, PathBuf::from);
    EngineConfig::new(data_dir, download_dir)
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "Show Rilldown", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause-all", "Pause all", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume-all", "Resume all", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &pause, &resume])
        .separator()
        .items(&[&settings_item, &quit])
        .build()?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(branding::APP_NAME)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "pause-all" => {
                let engine = app.state::<AppState>().engine.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = engine.pause_all().await;
                });
            }
            "resume-all" => {
                let engine = app.state::<AppState>().engine.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = engine.resume_all().await;
                });
            }
            "settings" => {
                show_main_window(app);
                let _ = app.emit("ui://settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

fn spawn_clipboard_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(Duration::from_millis(900)).await;
            let enabled = app.try_state::<AppState>().is_some_and(|state| {
                state
                    .settings
                    .lock()
                    .expect("settings lock")
                    .clipboard_monitor
            });
            if !enabled {
                continue;
            }
            let Ok(text) = app.clipboard().read_text() else {
                continue;
            };
            let text = text.trim().to_string();
            if text == last
                || text.len() > 2048
                || !(text.starts_with("http://") || text.starts_with("https://"))
            {
                continue;
            }
            last.clone_from(&text);
            let _ = app.emit("clipboard://url", text);
        }
    });
}

/// Build and run the desktop application.
#[allow(clippy::too_many_lines)]
pub fn run() {
    let config = engine_config();
    let engine = Engine::new(config).expect("failed to open the Rilldown database");
    let settings = AppSettings::load(&engine);
    settings.apply(&engine);
    let state = AppState::new(engine, settings);
    let engine_for_events = state.engine.clone();

    let builder = tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            let data_dir = handle.state::<AppState>().engine.config().data_dir.clone();
            bridge::clear_stale(&data_dir);
            match bridge::start(handle.clone()) {
                Ok(bridge) => {
                    tracing::info!(port = bridge.port(), "browser bridge listening");
                    app.manage(bridge);
                }
                Err(error) => {
                    tracing::warn!(%error, "browser bridge did not start");
                }
            }

            build_tray(&handle)?;
            spawn_clipboard_monitor(handle.clone());

            let mut events = engine_for_events.subscribe();
            let event_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            if let rilldown_engine::EngineEvent::Completed { path, .. } = &event {
                                let name = path
                                    .file_name()
                                    .and_then(std::ffi::OsStr::to_str)
                                    .unwrap_or("Download complete");
                                notify(&event_handle, "Download complete", name);
                            }
                            let _ = event_handle.emit("engine://event", &event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "engine event stream lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray =
                    window
                        .app_handle()
                        .try_state::<AppState>()
                        .is_some_and(|state| {
                            state.settings.lock().expect("settings lock").close_to_tray
                        });
                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            engine_info,
            get_settings,
            save_settings,
            choose_directory,
            list_downloads,
            add_download,
            add_downloads,
            pause_download,
            resume_download,
            cancel_download,
            remove_download,
            pause_all,
            resume_all,
            open_download,
            reveal_download,
            open_downloads_folder,
            list_queues,
            save_queue,
            remove_queue,
            browser_host_status,
            check_update,
            install_update,
            install_browser_host,
            uninstall_browser_host,
            open_extension_folder,
            tools_status,
            install_tool,
            media_probe,
            media_start,
            media_jobs,
            media_cancel,
            media_remove
        ]);

    builder
        .run(tauri::generate_context!())
        .expect("failed to run Rilldown");
}

#[cfg(test)]
mod tests {
    use rilldown_engine::DownloadState;

    use super::*;

    fn settings() -> AppSettings {
        AppSettings {
            download_dir: PathBuf::from("/tmp/rilldown-tests"),
            ..AppSettings::default()
        }
    }

    #[test]
    fn add_input_becomes_a_normalized_spec() {
        let input = AddInput {
            url: "  https://example.com/file.zip  ".to_string(),
            filename: Some("report.zip".to_string()),
            save_dir: None,
            connections: Some(4),
            headers: BTreeMap::new(),
            cookies: None,
            referrer: None,
            user_agent: None,
            proxy: None,
            speed_limit_bps: None,
            checksum: None,
            queue_id: None,
        };
        let spec = build_spec(input, &settings()).unwrap();
        assert_eq!(spec.url.as_str(), "https://example.com/file.zip");
        assert_eq!(spec.filename.as_deref(), Some("report.zip"));
        assert_eq!(spec.connections, 4);
        assert_eq!(spec.queue_id.as_deref(), Some(DEFAULT_QUEUE_ID));
        assert_eq!(spec.save_dir, PathBuf::from("/tmp/rilldown-tests"));
    }

    #[test]
    fn add_input_rejects_non_http_schemes() {
        let input = AddInput {
            url: "ftp://example.com/file".to_string(),
            filename: None,
            save_dir: None,
            connections: None,
            headers: BTreeMap::new(),
            cookies: None,
            referrer: None,
            user_agent: None,
            proxy: None,
            speed_limit_bps: None,
            checksum: None,
            queue_id: None,
        };
        let mut spec = build_spec(input, &settings()).unwrap();
        assert!(spec.normalize().is_err());
    }

    #[test]
    fn download_state_is_reported_as_snake_case() {
        let encoded = serde_json::to_string(&DownloadState::Downloading).unwrap();
        assert_eq!(encoded, "\"downloading\"");
    }
}
