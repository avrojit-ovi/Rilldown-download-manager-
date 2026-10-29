import { Check, Download, FolderOpen, Loader2, ShieldCheck, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import {
  api,
  BROWSER_APP_INFO,
  type AppSettings,
  type Queue,
  type ToolStatus,
  type UpdateInfo,
} from "@/lib/tauri";
import { useDownloadsStore } from "@/stores/downloads";
import { useUiStore } from "@/stores/ui";

interface FormState {
  downloadDir: string;
  connections: string;
  maxConcurrent: string;
  speedLimitKib: string;
  proxy: string;
  userAgent: string;
  clipboardMonitor: boolean;
  closeToTray: boolean;
  notifications: boolean;
}

function toForm(settings: AppSettings): FormState {
  return {
    downloadDir: settings.downloadDir,
    connections: String(settings.connections),
    maxConcurrent: String(settings.maxConcurrent),
    speedLimitKib:
      settings.speedLimitBps && settings.speedLimitBps > 0
        ? String(Math.round(settings.speedLimitBps / 1024))
        : "",
    proxy: settings.proxy ?? "",
    userAgent: settings.userAgent ?? "",
    clipboardMonitor: settings.clipboardMonitor,
    closeToTray: settings.closeToTray,
    notifications: settings.notifications,
  };
}

function parseCount(value: string): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : 0;
}

function slugify(name: string): string {
  return name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9-_]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

export function SettingsDialog() {
  const { t } = useTranslation();
  const dialog = useUiStore((state) => state.dialog);
  const closeDialog = useUiStore((state) => state.closeDialog);
  const settings = useDownloadsStore((state) => state.settings);
  const queues = useDownloadsStore((state) => state.queues);
  const tools = useDownloadsStore((state) => state.tools);
  const host = useDownloadsStore((state) => state.host);
  const setSettings = useDownloadsStore((state) => state.setSettings);
  const setQueues = useDownloadsStore((state) => state.setQueues);
  const setTools = useDownloadsStore((state) => state.setTools);
  const refreshSettings = useDownloadsStore((state) => state.refreshSettings);
  const refreshTools = useDownloadsStore((state) => state.refreshTools);
  const refreshHost = useDownloadsStore((state) => state.refreshHost);
  const refreshQueues = useDownloadsStore((state) => state.refreshQueues);

  const open = dialog === "settings";
  const [form, setForm] = useState<FormState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [installing, setInstalling] = useState<string | null>(null);
  const [toolError, setToolError] = useState<string | null>(null);
  const [hostError, setHostError] = useState<string | null>(null);
  const [hostBusy, setHostBusy] = useState(false);
  const [queueName, setQueueName] = useState("");
  const [queueLimit, setQueueLimit] = useState("2");
  const [queueError, setQueueError] = useState<string | null>(null);
  const [queueBusy, setQueueBusy] = useState<string | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);

  // Clear the editable copy when the dialog closes so reopening picks up the
  // latest saved settings without discarding edits on every store refresh.
  useEffect(() => {
    if (!open) {
      setForm(null);
      setSaved(false);
      setError(null);
      return;
    }
    void refreshSettings();
    void refreshTools();
    void refreshHost();
    void refreshQueues();
  }, [open, refreshSettings, refreshTools, refreshHost, refreshQueues]);

  useEffect(() => {
    if (open && !form && settings) {
      setForm(toForm(settings));
    }
  }, [open, form, settings]);

  const update = useCallback(<Key extends keyof FormState>(key: Key, value: FormState[Key]) => {
    setSaved(false);
    setForm((current) => (current ? { ...current, [key]: value } : current));
  }, []);

  const browse = async () => {
    setError(null);
    try {
      const directory = await api.chooseDirectory();
      if (directory) {
        update("downloadDir", directory);
      }
    } catch (cause) {
      setError(messageOf(cause));
    }
  };

  const save = async () => {
    if (!form) {
      return;
    }
    const connections = parseCount(form.connections);
    const maxConcurrent = parseCount(form.maxConcurrent);
    if (connections < 1 || connections > 32) {
      setError(t("settings.connectionsError", { defaultValue: "Connections must be between 1 and 32." }));
      return;
    }
    if (maxConcurrent < 1 || maxConcurrent > 32) {
      setError(
        t("settings.concurrencyError", {
          defaultValue: "Concurrent downloads must be between 1 and 32.",
        }),
      );
      return;
    }
    const kib = form.speedLimitKib.trim();
    const parsedLimit = kib === "" ? null : Number.parseFloat(kib);
    if (parsedLimit !== null && (!Number.isFinite(parsedLimit) || parsedLimit < 0)) {
      setError(t("settings.speedError", { defaultValue: "Speed limit must be a positive number of KiB/s." }));
      return;
    }
    if (!form.downloadDir.trim()) {
      setError(t("settings.folderError", { defaultValue: "Choose a download folder." }));
      return;
    }

    const payload: AppSettings = {
      downloadDir: form.downloadDir.trim(),
      connections,
      maxConcurrent,
      speedLimitBps: parsedLimit === null ? null : Math.round(parsedLimit * 1024),
      proxy: form.proxy.trim() === "" ? null : form.proxy.trim(),
      userAgent: form.userAgent.trim() === "" ? null : form.userAgent.trim(),
      clipboardMonitor: form.clipboardMonitor,
      closeToTray: form.closeToTray,
      notifications: form.notifications,
    };

    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const result = await api.saveSettings(payload);
      setSettings(result);
      setForm(toForm(result));
      setSaved(true);
    } catch (cause) {
      setError(messageOf(cause));
    } finally {
      setSaving(false);
    }
  };

  const installTool = async (tool: "ffmpeg" | "yt-dlp") => {
    setInstalling(tool);
    setToolError(null);
    try {
      const result = await api.installTool(tool);
      setTools(result);
    } catch (cause) {
      setToolError(messageOf(cause));
    } finally {
      setInstalling(null);
    }
  };

  const installHost = async () => {
    setHostBusy(true);
    setHostError(null);
    try {
      await api.installBrowserHost();
      await refreshHost();
    } catch (cause) {
      setHostError(messageOf(cause));
    } finally {
      setHostBusy(false);
    }
  };

  const uninstallHost = async () => {
    setHostBusy(true);
    setHostError(null);
    try {
      await api.uninstallBrowserHost();
      await refreshHost();
    } catch (cause) {
      setHostError(messageOf(cause));
    } finally {
      setHostBusy(false);
    }
  };

  const openExtension = async () => {
    setHostError(null);
    try {
      await api.openExtensionFolder();
    } catch (cause) {
      setHostError(messageOf(cause));
    }
  };

  const addQueue = async () => {
    const name = queueName.trim();
    if (!name) {
      setQueueError(t("settings.queueNameError", { defaultValue: "Give the queue a name." }));
      return;
    }
    const id = slugify(name);
    if (!id) {
      setQueueError(
        t("settings.queueIdError", { defaultValue: "Use at least one letter or number in the name." }),
      );
      return;
    }
    if (queues.some((queue) => queue.id === id)) {
      setQueueError(t("settings.queueExists", { defaultValue: "A queue with that name already exists." }));
      return;
    }
    const limit = parseCount(queueLimit);
    if (limit < 0 || limit > 32) {
      setQueueError(
        t("settings.queueLimitError", {
          defaultValue: "Queue concurrency must be between 0 (global only) and 32.",
        }),
      );
      return;
    }
    setQueueBusy("add");
    setQueueError(null);
    try {
      const result = await api.saveQueue({
        id,
        name,
        maxConcurrent: limit,
        speedLimitBps: null,
      });
      setQueues(result);
      setQueueName("");
      setQueueLimit("2");
    } catch (cause) {
      setQueueError(messageOf(cause));
    } finally {
      setQueueBusy(null);
    }
  };

  const removeQueue = async (queue: Queue) => {
    setQueueBusy(queue.id);
    setQueueError(null);
    try {
      const result = await api.removeQueue(queue.id);
      setQueues(result);
    } catch (cause) {
      setQueueError(messageOf(cause));
    } finally {
      setQueueBusy(null);
    }
  };

  const footer = (
    <>
      <span className="mr-auto text-[11px] text-muted">
        {saved ? (
          <span className="inline-flex items-center gap-1 text-accent">
            <Check className="h-3.5 w-3.5" aria-hidden="true" />
            {t("settings.saved", { defaultValue: "Saved" })}
          </span>
        ) : null}
      </span>
      <Button variant="secondary" onClick={closeDialog}>
        {t("settings.close", { defaultValue: "Close" })}
      </Button>
      <Button variant="primary" onClick={save} disabled={saving || !form}>
        {saving ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" /> : null}
        {t("settings.save", { defaultValue: "Save changes" })}
      </Button>
    </>
  );

  return (
    <Dialog
      open={open}
      title={t("settings.title", { defaultValue: "Settings" })}
      description={t("settings.description", {
        defaultValue: "Engine limits, network identity, tools, and browser integration.",
      })}
      onClose={closeDialog}
      footer={footer}
      className="max-w-[680px]"
    >
      {!form ? (
        <p className="py-6 text-center text-[13px] text-muted">
          {t("settings.loading", { defaultValue: "Loading settings…" })}
        </p>
      ) : (
        <div className="space-y-6">
          {error ? (
            <p role="alert" className="rounded-lg border border-danger/30 bg-danger/5 px-3 py-2 text-[12px] text-danger">
              {error}
            </p>
          ) : null}

          <Section title={t("settings.downloads", { defaultValue: "Downloads" })}>
            <Field label={t("settings.folder", { defaultValue: "Download folder" })}>
              <div className="flex gap-2">
                <Input
                  value={form.downloadDir}
                  onChange={(event) => update("downloadDir", event.target.value)}
                  aria-label={t("settings.folder", { defaultValue: "Download folder" })}
                />
                <Button variant="secondary" onClick={browse}>
                  <FolderOpen className="h-4 w-4" aria-hidden="true" />
                  {t("settings.browse", { defaultValue: "Browse" })}
                </Button>
              </div>
            </Field>

            <div className="grid gap-3 sm:grid-cols-3">
              <Field label={t("settings.connections", { defaultValue: "Connections" })}>
                <Input
                  type="number"
                  min={1}
                  max={32}
                  value={form.connections}
                  onChange={(event) => update("connections", event.target.value)}
                />
              </Field>
              <Field label={t("settings.concurrency", { defaultValue: "Active downloads" })}>
                <Input
                  type="number"
                  min={1}
                  max={32}
                  value={form.maxConcurrent}
                  onChange={(event) => update("maxConcurrent", event.target.value)}
                />
              </Field>
              <Field
                label={t("settings.speedLimit", { defaultValue: "Speed limit (KiB/s)" })}
                hint={t("settings.speedLimitHint", { defaultValue: "Empty means unlimited." })}
              >
                <Input
                  type="number"
                  min={0}
                  placeholder={t("settings.unlimited", { defaultValue: "Unlimited" })}
                  value={form.speedLimitKib}
                  onChange={(event) => update("speedLimitKib", event.target.value)}
                />
              </Field>
            </div>
          </Section>

          <Section title={t("settings.network", { defaultValue: "Network" })}>
            <Field
              label={t("settings.proxy", { defaultValue: "Proxy URL" })}
              hint={t("settings.proxyHint", {
                defaultValue: "For example socks5://127.0.0.1:1080. Leave empty for a direct connection.",
              })}
            >
              <Input
                value={form.proxy}
                placeholder="socks5://127.0.0.1:1080"
                onChange={(event) => update("proxy", event.target.value)}
              />
            </Field>
            <Field
              label={t("settings.userAgent", { defaultValue: "Custom user agent" })}
              hint={t("settings.userAgentHint", {
                defaultValue: "Sent with every new download unless the request overrides it.",
              })}
            >
              <Input
                value={form.userAgent}
                placeholder="Rilldown/0.1.0"
                onChange={(event) => update("userAgent", event.target.value)}
              />
            </Field>
          </Section>

          <Section title={t("settings.behavior", { defaultValue: "Behavior" })}>
            <Toggle
              checked={form.clipboardMonitor}
              onChange={(value) => update("clipboardMonitor", value)}
              label={t("settings.clipboard", { defaultValue: "Watch the clipboard for links" })}
              hint={t("settings.clipboardHint", {
                defaultValue: "Offer to download new http(s) links copied anywhere on the system.",
              })}
            />
            <Toggle
              checked={form.closeToTray}
              onChange={(value) => update("closeToTray", value)}
              label={t("settings.closeToTray", { defaultValue: "Keep running in the tray" })}
              hint={t("settings.closeToTrayHint", {
                defaultValue: "Closing the window hides it instead of quitting.",
              })}
            />
            <Toggle
              checked={form.notifications}
              onChange={(value) => update("notifications", value)}
              label={t("settings.notifications", { defaultValue: "Desktop notifications" })}
              hint={t("settings.notificationsHint", {
                defaultValue: "Show a notification when a download or media grab finishes.",
              })}
            />
          </Section>

          <Section title={t("settings.tools", { defaultValue: "Tools" })}>
            <p className="text-[12px] leading-5 text-muted">
              {t("settings.toolsHint", {
                defaultValue:
                  "ffmpeg merges HLS/DASH streams; yt-dlp extracts media from sites. They are looked up on PATH first, then downloaded on first use.",
              })}
            </p>
            <div className="space-y-2">
              <ToolRow
                tool="ffmpeg"
                label="ffmpeg"
                status={tools.find((entry) => entry.tool === "ffmpeg")}
                installing={installing === "ffmpeg"}
                onInstall={() => void installTool("ffmpeg")}
                installLabel={t("settings.install", { defaultValue: "Install" })}
                missingLabel={t("settings.notInstalled", { defaultValue: "Not installed" })}
                managedLabel={t("settings.managed", { defaultValue: "Managed" })}
              />
              <ToolRow
                tool="yt-dlp"
                label="yt-dlp"
                status={tools.find((entry) => entry.tool === "yt-dlp")}
                installing={installing === "yt-dlp"}
                onInstall={() => void installTool("yt-dlp")}
                installLabel={t("settings.install", { defaultValue: "Install" })}
                missingLabel={t("settings.notInstalled", { defaultValue: "Not installed" })}
                managedLabel={t("settings.managed", { defaultValue: "Managed" })}
              />
            </div>
            {toolError ? (
              <p role="alert" className="text-[12px] text-danger">
                {toolError}
              </p>
            ) : null}
          </Section>

          <Section title={t("settings.browser", { defaultValue: "Browser integration" })}>
            <div className="flex flex-wrap items-center gap-2">
              <span
                className={
                  host?.installed
                    ? "inline-flex items-center gap-1.5 rounded-full bg-accent-soft px-2.5 py-1 text-[11px] font-medium text-accent"
                    : "inline-flex items-center gap-1.5 rounded-full bg-surface-2 px-2.5 py-1 text-[11px] font-medium text-muted"
                }
              >
                <ShieldCheck className="h-3.5 w-3.5" aria-hidden="true" />
                {host?.installed
                  ? t("settings.hostInstalled", { defaultValue: "Native bridge installed" })
                  : t("settings.hostMissing", { defaultValue: "Native bridge not installed" })}
              </span>
              <Button variant="secondary" size="sm" onClick={installHost} disabled={hostBusy}>
                {hostBusy ? <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" /> : null}
                {t("settings.installHost", { defaultValue: "Install bridge" })}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={uninstallHost}
                disabled={hostBusy || !host?.installed}
              >
                {t("settings.uninstallHost", { defaultValue: "Remove bridge" })}
              </Button>
              <Button variant="ghost" size="sm" onClick={openExtension}>
                <FolderOpen className="h-3.5 w-3.5" aria-hidden="true" />
                {t("settings.openExtension", { defaultValue: "Open extension folder" })}
              </Button>
            </div>
            <p className="text-[12px] leading-5 text-muted">
              {t("settings.browserHint", {
                defaultValue:
                  "Install the native bridge, then load the extension folder as an unpacked extension in Chrome, Edge, or Brave, or via about:debugging in Firefox.",
              })}
            </p>
            <dl className="grid gap-1.5 text-[11px] text-muted">
              <div className="flex gap-2">
                <dt className="w-32 shrink-0 font-medium text-ink">
                  {t("settings.chromeId", { defaultValue: "Chrome extension id" })}
                </dt>
                <dd className="truncate font-mono">{host?.chromeExtensionId ?? "…"}</dd>
              </div>
              <div className="flex gap-2">
                <dt className="w-32 shrink-0 font-medium text-ink">
                  {t("settings.firefoxId", { defaultValue: "Firefox add-on id" })}
                </dt>
                <dd className="truncate font-mono">{host?.firefoxExtensionId ?? "…"}</dd>
              </div>
            </dl>
            {hostError ? (
              <p role="alert" className="text-[12px] text-danger">
                {hostError}
              </p>
            ) : null}
          </Section>

          <Section title={t("settings.queues", { defaultValue: "Queues" })}>
            <ul className="space-y-1.5">
              {queues.map((queue) => (
                <li
                  key={queue.id}
                  className="flex items-center gap-3 rounded-lg border border-line bg-surface-2/40 px-3 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[12.5px] font-medium text-ink">{queue.name}</p>
                    <p className="text-[11px] text-muted">
                      {queue.max_concurrent === 0
                        ? t("settings.globalOnly", { defaultValue: "Global limit only" })
                        : t("settings.queueConcurrent", {
                            defaultValue: "{{count}} at a time",
                            count: queue.max_concurrent,
                          })}
                    </p>
                  </div>
                  {queue.id === "main" ? (
                    <span className="text-[10px] font-semibold uppercase tracking-wider text-muted">
                      {t("settings.builtIn", { defaultValue: "Built-in" })}
                    </span>
                  ) : (
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={t("settings.removeQueue", {
                        defaultValue: "Remove {{name}}",
                        name: queue.name,
                      })}
                      onClick={() => void removeQueue(queue)}
                      disabled={queueBusy === queue.id}
                    >
                      {queueBusy === queue.id ? (
                        <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                      ) : (
                        <Trash2 className="h-4 w-4" aria-hidden="true" />
                      )}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
            <div className="flex flex-wrap items-end gap-2">
              <Field label={t("settings.queueName", { defaultValue: "New queue" })}>
                <Input
                  value={queueName}
                  placeholder={t("settings.queueNamePlaceholder", { defaultValue: "Night downloads" })}
                  onChange={(event) => setQueueName(event.target.value)}
                  className="w-[200px]"
                />
              </Field>
              <Field label={t("settings.queueLimit", { defaultValue: "At a time" })}>
                <Input
                  type="number"
                  min={0}
                  max={32}
                  value={queueLimit}
                  onChange={(event) => setQueueLimit(event.target.value)}
                  className="w-[88px]"
                />
              </Field>
              <Button variant="secondary" onClick={addQueue} disabled={queueBusy === "add"}>
                {queueBusy === "add" ? (
                  <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                ) : (
                  <Download className="h-4 w-4" aria-hidden="true" />
                )}
                {t("settings.addQueue", { defaultValue: "Add queue" })}
              </Button>
            </div>
            {queueError ? (
              <p role="alert" className="text-[12px] text-danger">
                {queueError}
              </p>
            ) : null}
          </Section>

          <Section title={t("settings.updates", { defaultValue: "Updates" })}>
            <div className="flex flex-wrap items-center gap-2">
              <Button
                variant="secondary"
                onClick={async () => {
                  setUpdateBusy(true);
                  setUpdateMessage(null);
                  try {
                    const result = await api.checkUpdate();
                    setUpdateInfo(result);
                    setUpdateMessage(
                      result.available
                        ? t("settings.updateAvailable", {
                            defaultValue: "Version {{version}} is available.",
                            version: result.version ?? "",
                          })
                        : t("settings.upToDate", {
                            defaultValue: "You are running the latest version.",
                          }),
                    );
                  } catch (cause) {
                    setUpdateMessage(messageOf(cause));
                  } finally {
                    setUpdateBusy(false);
                  }
                }}
                disabled={updateBusy}
              >
                {updateBusy ? (
                  <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                ) : (
                  <ShieldCheck className="h-4 w-4" aria-hidden="true" />
                )}
                {t("settings.checkUpdates", { defaultValue: "Check for updates" })}
              </Button>
              {updateInfo?.available ? (
                <Button
                  variant="primary"
                  onClick={async () => {
                    setUpdateBusy(true);
                    setUpdateMessage(null);
                    try {
                      await api.installUpdate();
                    } catch (cause) {
                      setUpdateMessage(messageOf(cause));
                      setUpdateBusy(false);
                    }
                  }}
                  disabled={updateBusy}
                >
                  {t("settings.installUpdate", { defaultValue: "Install update" })}
                </Button>
              ) : null}
            </div>
            {updateMessage ? (
              <p className="text-[11px] leading-5 text-muted">{updateMessage}</p>
            ) : null}
            {updateInfo?.available && updateInfo.notes ? (
              <p className="line-clamp-3 whitespace-pre-line text-[11px] leading-5 text-muted">
                {updateInfo.notes}
              </p>
            ) : null}
          </Section>

          <p className="border-t border-line pt-4 text-[11px] text-muted">
            {t("settings.about", {
              defaultValue: "Rilldown {{version}} · MIT",
              version: BROWSER_APP_INFO.version,
            })}
          </p>
        </div>
      )}
    </Dialog>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-3">
      <h3 className="text-[9.5px] font-semibold uppercase tracking-[0.16em] text-muted/80">{title}</h3>
      {children}
    </section>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="block min-w-0 flex-1 space-y-1.5">
      <span className="block text-[11.5px] font-medium text-ink">{label}</span>
      {children}
      {hint ? <span className="block text-[10.5px] leading-4 text-muted">{hint}</span> : null}
    </label>
  );
}

function Toggle({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  hint: string;
}) {
  return (
    <div className="flex items-start gap-3 rounded-lg border border-line bg-surface-2/30 px-3 py-2.5">
      <div className="min-w-0 flex-1">
        <p className="text-[12.5px] font-medium text-ink">{label}</p>
        <p className="mt-0.5 text-[11px] leading-4 text-muted">{hint}</p>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        onClick={() => onChange(!checked)}
        className={
          checked
            ? "relative h-5 w-9 shrink-0 rounded-full bg-accent transition-colors"
            : "relative h-5 w-9 shrink-0 rounded-full bg-line transition-colors"
        }
      >
        <span
          className={
            checked
              ? "absolute right-0.5 top-0.5 h-4 w-4 rounded-full bg-accent-contrast transition-transform"
              : "absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-surface transition-transform"
          }
        />
      </button>
    </div>
  );
}

function ToolRow({
  tool,
  label,
  status,
  installing,
  onInstall,
  installLabel,
  missingLabel,
  managedLabel,
}: {
  tool: ToolStatus["tool"];
  label: string;
  status: ToolStatus | undefined;
  installing: boolean;
  onInstall: () => void;
  installLabel: string;
  missingLabel: string;
  managedLabel: string;
}) {
  const { t } = useTranslation();
  const available = Boolean(status?.path);
  return (
    <div className="flex items-center gap-3 rounded-lg border border-line bg-surface-2/40 px-3 py-2">
      <div className="min-w-0 flex-1">
        <p className="text-[12.5px] font-medium text-ink">{label}</p>
        <p className="truncate text-[10.5px] text-muted">
          {available
            ? [status?.version, status?.managed ? managedLabel : null, status?.path]
                .filter(Boolean)
                .join(" · ")
            : missingLabel}
        </p>
      </div>
      <Button
        variant={available ? "ghost" : "secondary"}
        size="sm"
        onClick={onInstall}
        disabled={installing}
        aria-label={t("settings.installTool", {
          defaultValue: "Install {{tool}}",
          tool,
        })}
      >
        {installing ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
        ) : (
          <Download className="h-3.5 w-3.5" aria-hidden="true" />
        )}
        {installing ? t("settings.installing", { defaultValue: "Installing…" }) : installLabel}
      </Button>
    </div>
  );
}

function messageOf(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
