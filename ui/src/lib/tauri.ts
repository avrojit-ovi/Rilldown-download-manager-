import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface AppInfo {
  name: string;
  version: string;
  identifier: string;
  engineVersion: string;
  platform: string;
  arch: string;
}

export const BROWSER_APP_INFO: AppInfo = {
  name: "Rilldown",
  version: "0.1.0",
  identifier: "app.rilldown.desktop",
  engineVersion: "0.1.0",
  platform: "browser",
  arch: "web",
};

export type DownloadState =
  | "queued"
  | "probing"
  | "downloading"
  | "paused"
  | "merging"
  | "verifying"
  | "completed"
  | "failed"
  | "cancelled";

export interface SegmentProgress {
  index: number;
  start_byte: number;
  end_byte: number;
  downloaded: number;
  speed_bps: number;
  active: boolean;
}

export interface DownloadProgress {
  id: string;
  state: DownloadState;
  downloaded_bytes: number;
  total_bytes: number | null;
  speed_bps: number;
  eta_seconds: number | null;
  connections: SegmentProgress[];
}

export interface DownloadRecord {
  id: string;
  url: string;
  final_url: string | null;
  save_dir: string;
  filename: string;
  custom_filename: boolean;
  state: DownloadState;
  total_bytes: number | null;
  downloaded_bytes: number;
  resumable: boolean;
  etag: string | null;
  last_modified: string | null;
  content_type: string | null;
  error: string | null;
  checksum: { algorithm: string; expected_hex: string } | null;
  checksum_actual: string | null;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
  queue_id: string | null;
  category: string | null;
  connections: number;
  speed_limit_bps: number | null;
  headers: Record<string, string>;
  cookies: string | null;
  referrer: string | null;
  user_agent: string | null;
  proxy: string | null;
  mirrors: string[];
}

export type EngineEvent =
  | { type: "progress"; progress: DownloadProgress }
  | { type: "state_changed"; id: string; state: DownloadState; error: string | null }
  | { type: "completed"; id: string; path: string }
  | { type: "log"; level: string; message: string };

export interface Queue {
  id: string;
  name: string;
  max_concurrent: number;
  speed_limit_bps: number | null;
  created_at: string;
}

export interface AppSettings {
  downloadDir: string;
  connections: number;
  maxConcurrent: number;
  speedLimitBps: number | null;
  proxy: string | null;
  userAgent: string | null;
  clipboardMonitor: boolean;
  closeToTray: boolean;
  notifications: boolean;
}

export interface AddDownloadInput {
  url: string;
  filename?: string;
  saveDir?: string;
  connections?: number;
  headers?: Record<string, string>;
  cookies?: string;
  referrer?: string;
  userAgent?: string;
  proxy?: string;
  speedLimitBps?: number | null;
  queueId?: string;
}

export interface BatchOutcome {
  url: string;
  ok: boolean;
  record: DownloadRecord | null;
  error: string | null;
}

export interface MediaFormat {
  id: string;
  label: string;
  ext: string;
  resolution: string | null;
  width: number | null;
  height: number | null;
  fps: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
  filesize: number | null;
  note: string | null;
}

export interface MediaInfo {
  title: string;
  durationSeconds: number | null;
  thumbnail: string | null;
  uploader: string | null;
  webpageUrl: string | null;
  formats: MediaFormat[];
}

export type MediaJobState = "queued" | "downloading" | "completed" | "failed" | "cancelled";

export interface MediaJob {
  id: string;
  url: string;
  title: string;
  formatId: string | null;
  state: MediaJobState;
  percent: number | null;
  downloadedBytes: number;
  totalBytes: number | null;
  speedBps: number;
  etaSeconds: number | null;
  stage: string;
  error: string | null;
  path: string | null;
  createdAt: number;
}

export interface ToolStatus {
  tool: "ffmpeg" | "yt-dlp";
  path: string | null;
  version: string | null;
  managed: boolean;
}

export interface UpdateInfo {
  available: boolean;
  version: string | null;
  notes: string | null;
  date: string | null;
}

export interface BrowserHostStatus {
  installed: boolean;
  executable: string | null;
  chromeExtensionId: string;
  firefoxExtensionId: string;
  manifests: string[];
}

export interface BrowserHostInstall {
  executable: string;
  chromeExtensionId: string;
  firefoxExtensionId: string;
  manifests: string[];
  browsers: string[];
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauri()) {
    return BROWSER_APP_INFO;
  }
  return invoke<AppInfo>("app_info");
}

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error("Rilldown's desktop engine is not connected in a browser tab.");
  }
  return invoke<T>(name, args);
}

export const api = {
  getSettings: () => command<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => command<AppSettings>("save_settings", { settings }),
  chooseDirectory: () => command<string | null>("choose_directory"),
  listDownloads: () => command<DownloadRecord[]>("list_downloads"),
  addDownload: (input: AddDownloadInput) => command<DownloadRecord>("add_download", { input }),
  addDownloads: (inputs: AddDownloadInput[]) =>
    command<BatchOutcome[]>("add_downloads", { inputs }),
  pauseDownload: (id: string) => command<void>("pause_download", { id }),
  resumeDownload: (id: string) => command<void>("resume_download", { id }),
  cancelDownload: (id: string) => command<void>("cancel_download", { id }),
  removeDownload: (id: string, deleteFile: boolean) =>
    command<void>("remove_download", { id, deleteFile }),
  pauseAll: () => command<number>("pause_all"),
  resumeAll: () => command<number>("resume_all"),
  openDownload: (id: string) => command<void>("open_download", { id }),
  revealDownload: (id: string) => command<void>("reveal_download", { id }),
  openDownloadsFolder: () => command<void>("open_downloads_folder"),
  listQueues: () => command<Queue[]>("list_queues"),
  saveQueue: (queue: {
    id: string;
    name: string;
    maxConcurrent: number;
    speedLimitBps: number | null;
  }) => command<Queue[]>("save_queue", { queue }),
  removeQueue: (id: string) => command<Queue[]>("remove_queue", { id }),
  checkUpdate: () => command<UpdateInfo>("check_update"),
  installUpdate: () => command<void>("install_update"),
  browserHostStatus: () => command<BrowserHostStatus>("browser_host_status"),
  installBrowserHost: () => command<BrowserHostInstall>("install_browser_host"),
  uninstallBrowserHost: () => command<number>("uninstall_browser_host"),
  openExtensionFolder: () => command<string>("open_extension_folder"),
  toolsStatus: () => command<ToolStatus[]>("tools_status"),
  installTool: (tool: "ffmpeg" | "yt-dlp") => command<ToolStatus[]>("install_tool", { tool }),
  mediaProbe: (url: string) => command<MediaInfo>("media_probe", { url }),
  mediaStart: (input: {
    url: string;
    formatId?: string;
    filename?: string;
    referrer?: string;
  }) => command<MediaJob>("media_start", { input }),
  mediaJobs: () => command<MediaJob[]>("media_jobs"),
  mediaCancel: (id: string) => command<boolean>("media_cancel", { id }),
  mediaRemove: (id: string) => command<void>("media_remove", { id }),
};

export async function onEngineEvent(
  handler: (event: EngineEvent) => void,
): Promise<UnlistenFn | null> {
  if (!isTauri()) {
    return null;
  }
  return listen<EngineEvent>("engine://event", (event) => handler(event.payload));
}

export async function onMediaJob(handler: (job: MediaJob) => void): Promise<UnlistenFn | null> {
  if (!isTauri()) {
    return null;
  }
  return listen<MediaJob>("media://job", (event) => handler(event.payload));
}

export async function onClipboardUrl(
  handler: (url: string) => void,
): Promise<UnlistenFn | null> {
  if (!isTauri()) {
    return null;
  }
  return listen<string>("clipboard://url", (event) => handler(event.payload));
}

export async function onOpenSettings(handler: () => void): Promise<UnlistenFn | null> {
  if (!isTauri()) {
    return null;
  }
  return listen("ui://settings", () => handler());
}
