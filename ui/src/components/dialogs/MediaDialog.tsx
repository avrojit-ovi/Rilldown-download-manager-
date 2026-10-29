import { Check, Download, Film, Loader2, Sparkles } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { api, type MediaFormat, type MediaInfo, type ToolStatus } from "@/lib/tauri";
import { cn, formatBytes } from "@/lib/utils";
import { useDownloadsStore } from "@/stores/downloads";
import { useUiStore } from "@/stores/ui";

function messageOf(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function formatDuration(seconds: number | null): string | null {
  if (seconds === null || !Number.isFinite(seconds) || seconds <= 0) {
    return null;
  }
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remainder = total % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(remainder).padStart(2, "0")}`;
  }
  return `${minutes}:${String(remainder).padStart(2, "0")}`;
}

function isHttpUrl(value: string): boolean {
  try {
    const parsed = new URL(value.trim());
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

function formatDetail(format: MediaFormat): string {
  const parts = [format.label];
  if (format.filesize !== null) {
    parts.push(formatBytes(format.filesize));
  }
  return parts.filter(Boolean).join(" · ");
}

export function MediaDialog() {
  const { t } = useTranslation();
  const dialog = useUiStore((state) => state.dialog);
  const closeDialog = useUiStore((state) => state.closeDialog);
  const tools = useDownloadsStore((state) => state.tools);
  const setTools = useDownloadsStore((state) => state.setTools);
  const refreshMedia = useDownloadsStore((state) => state.refreshMedia);

  const [url, setUrl] = useState("");
  const [info, setInfo] = useState<MediaInfo | null>(null);
  const [selectedFormat, setSelectedFormat] = useState<string | undefined>(undefined);
  const [probing, setProbing] = useState(false);
  const [starting, setStarting] = useState(false);
  const [installing, setInstalling] = useState<"yt-dlp" | "ffmpeg" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const ytDlp = tools.find((tool) => tool.tool === "yt-dlp");
  const ffmpeg = tools.find((tool) => tool.tool === "ffmpeg");
  const missingYtDlp = !ytDlp?.path;
  const busy = probing || starting || installing !== null;

  const reset = () => {
    setUrl("");
    setInfo(null);
    setSelectedFormat(undefined);
    setError(null);
  };

  const close = () => {
    closeDialog();
    reset();
  };

  const install = async (tool: "yt-dlp" | "ffmpeg") => {
    setInstalling(tool);
    setError(null);
    try {
      const next = await api.installTool(tool);
      setTools(next);
    } catch (installError) {
      setError(messageOf(installError));
    } finally {
      setInstalling(null);
    }
  };

  const inspect = async () => {
    if (!isHttpUrl(url) || probing || missingYtDlp) {
      return;
    }
    setProbing(true);
    setError(null);
    try {
      const result = await api.mediaProbe(url.trim());
      setInfo(result);
      setSelectedFormat(undefined);
    } catch (probeError) {
      setInfo(null);
      setError(messageOf(probeError));
    } finally {
      setProbing(false);
    }
  };

  const start = async () => {
    if (!info || starting) {
      return;
    }
    setStarting(true);
    setError(null);
    try {
      await api.mediaStart({
        url: info.webpageUrl ?? url.trim(),
        formatId: selectedFormat,
      });
      await refreshMedia();
      close();
    } catch (startError) {
      setError(messageOf(startError));
    } finally {
      setStarting(false);
    }
  };

  const videoFormats = info?.formats.filter((format) => format.videoCodec !== null) ?? [];
  const audioFormats = info?.formats.filter((format) => format.videoCodec === null) ?? [];
  const canInspect = isHttpUrl(url) && !missingYtDlp;

  const footer = (
    <>
      <Button variant="ghost" onClick={close} disabled={starting}>
        {t("common.cancel", { defaultValue: "Cancel" })}
      </Button>
      {info ? (
        <Button variant="primary" onClick={() => void start()} disabled={busy}>
          {starting ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          ) : (
            <Download className="h-4 w-4" aria-hidden="true" />
          )}
          {starting
            ? t("media.starting", { defaultValue: "Starting…" })
            : t("media.start", { defaultValue: "Grab media" })}
        </Button>
      ) : (
        <Button
          variant="primary"
          onClick={() => void inspect()}
          disabled={!canInspect || probing}
        >
          {probing ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          ) : (
            <Sparkles className="h-4 w-4" aria-hidden="true" />
          )}
          {probing
            ? t("media.inspecting", { defaultValue: "Inspecting…" })
            : t("media.inspect", { defaultValue: "Inspect" })}
        </Button>
      )}
    </>
  );

  return (
    <Dialog
      open={dialog === "media"}
      title={t("media.title", { defaultValue: "Grab media" })}
      description={t("media.description", {
        defaultValue:
          "Download HLS, DASH, or a page's best video with yt-dlp. ffmpeg merges the streams.",
      })}
      onClose={close}
      footer={footer}
      className="max-w-[620px]"
    >
      {missingYtDlp ? (
        <div className="rounded-xl border border-line bg-surface-2/60 p-4">
          <div className="flex items-start gap-3">
            <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
              <Film className="h-4 w-4" aria-hidden="true" />
            </span>
            <div className="min-w-0 flex-1">
              <h3 className="text-[13px] font-semibold text-ink">
                {t("media.setupTitle", { defaultValue: "One-time media setup" })}
              </h3>
              <p className="mt-1 text-[12px] leading-5 text-muted">
                {t("media.setupBody", {
                  defaultValue:
                    "Rilldown downloads yt-dlp and ffmpeg from their official release channels on first use, then keeps them in its data folder. Nothing is installed system-wide.",
                })}
              </p>
              <Button
                className="mt-3"
                variant="primary"
                size="sm"
                onClick={() => void install("yt-dlp")}
                disabled={installing !== null}
              >
                {installing === "yt-dlp" ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                ) : (
                  <Download className="h-3.5 w-3.5" aria-hidden="true" />
                )}
                {installing === "yt-dlp"
                  ? t("media.installing", { defaultValue: "Installing…" })
                  : t("media.installYtDlp", { defaultValue: "Install yt-dlp" })}
              </Button>
            </div>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          <label className="block">
            <span className="mb-1.5 block text-[11px] font-medium text-muted">
              {t("media.urlLabel", { defaultValue: "Page or stream URL" })}
            </span>
            <Input
              type="url"
              inputMode="url"
              autoFocus
              value={url}
              placeholder="https://example.com/watch?v=…"
              onChange={(event) => {
                setUrl(event.target.value);
                setInfo(null);
                setSelectedFormat(undefined);
                setError(null);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" && canInspect) {
                  event.preventDefault();
                  void inspect();
                }
              }}
              disabled={probing || starting}
            />
          </label>

          {info ? (
            <div className="space-y-4">
              <div className="flex gap-3 rounded-xl border border-line bg-surface-2/50 p-3">
                {info.thumbnail?.startsWith("https://") ? (
                  <img
                    src={info.thumbnail}
                    alt=""
                    referrerPolicy="no-referrer"
                    className="h-16 w-28 shrink-0 rounded-lg border border-line object-cover"
                  />
                ) : (
                  <span className="flex h-16 w-28 shrink-0 items-center justify-center rounded-lg border border-line bg-surface text-muted">
                    <Film className="h-5 w-5" aria-hidden="true" />
                  </span>
                )}
                <div className="min-w-0 flex-1">
                  <p className="line-clamp-2 text-[13px] font-medium leading-5 text-ink">
                    {info.title}
                  </p>
                  <p className="mt-1 truncate text-[11px] text-muted">
                    {[info.uploader, formatDuration(info.durationSeconds)]
                      .filter(Boolean)
                      .join(" · ") || t("media.unknownSource", { defaultValue: "Media source" })}
                  </p>
                </div>
              </div>

              <fieldset>
                <legend className="mb-2 text-[11px] font-medium text-muted">
                  {t("media.quality", { defaultValue: "Quality" })}
                </legend>
                <div className="space-y-1.5">
                  <FormatOption
                    checked={selectedFormat === undefined}
                    onSelect={() => setSelectedFormat(undefined)}
                    title={t("media.best", { defaultValue: "Best available" })}
                    detail={t("media.bestDetail", {
                      defaultValue: "Video plus audio, merged to MP4 when needed",
                    })}
                  />

                  {videoFormats.length > 0 ? (
                    <p className="px-1 pt-1 text-[10px] font-semibold uppercase tracking-wider text-muted/80">
                      {t("media.video", { defaultValue: "Video" })}
                    </p>
                  ) : null}
                  {videoFormats.map((format) => (
                    <FormatOption
                      key={format.id}
                      checked={selectedFormat === format.id}
                      onSelect={() => setSelectedFormat(format.id)}
                      title={format.resolution ?? format.label}
                      detail={formatDetail(format)}
                    />
                  ))}

                  {audioFormats.length > 0 ? (
                    <p className="px-1 pt-1 text-[10px] font-semibold uppercase tracking-wider text-muted/80">
                      {t("media.audioOnly", { defaultValue: "Audio only" })}
                    </p>
                  ) : null}
                  {audioFormats.map((format) => (
                    <FormatOption
                      key={format.id}
                      checked={selectedFormat === format.id}
                      onSelect={() => setSelectedFormat(format.id)}
                      title={format.label}
                      detail={formatDetail(format)}
                    />
                  ))}
                </div>
              </fieldset>

              {info.formats.length === 0 ? (
                <p className="text-[12px] text-muted">
                  {t("media.noFormats", {
                    defaultValue: "No individual formats were listed; Best available will be used.",
                  })}
                </p>
              ) : null}
            </div>
          ) : null}

          {!info ? (
            <p className="text-[11px] leading-5 text-muted/80">
              {t("media.inspectHint", {
                defaultValue:
                  "Inspect first to list the available qualities. Nothing is downloaded until you press Grab media.",
              })}
            </p>
          ) : null}
        </div>
      )}

      {error ? (
        <p
          role="alert"
          className="mt-4 rounded-lg border border-danger/25 bg-danger/5 px-3 py-2 text-[12px] leading-5 text-danger"
        >
          {error}
        </p>
      ) : null}

      {!missingYtDlp && !ffmpeg?.path ? (
        <div className="mt-4 flex items-start justify-between gap-3 rounded-lg border border-line bg-surface-2/50 px-3 py-2.5">
          <p className="text-[11px] leading-5 text-muted">
            {t("media.ffmpegMissing", {
              defaultValue:
                "ffmpeg is not installed. Rilldown needs it to merge separate audio and video streams.",
            })}
          </p>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => void install("ffmpeg")}
            disabled={installing !== null}
          >
            {installing === "ffmpeg" ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            ) : (
              <Download className="h-3.5 w-3.5" aria-hidden="true" />
            )}
            {t("media.installFfmpeg", { defaultValue: "Install ffmpeg" })}
          </Button>
        </div>
      ) : null}

      {ytDlp?.path || ffmpeg?.path ? (
        <ToolSummary tools={tools} />
      ) : null}
    </Dialog>
  );
}

function FormatOption({
  checked,
  onSelect,
  title,
  detail,
}: {
  checked: boolean;
  onSelect: () => void;
  title: string;
  detail: string;
}) {
  return (
    <label
      className={cn(
        "flex cursor-pointer items-center gap-3 rounded-lg border px-3 py-2 transition-colors",
        checked
          ? "border-accent/50 bg-accent-soft/60"
          : "border-line bg-surface hover:bg-surface-2",
      )}
    >
      <input
        type="radio"
        className="sr-only"
        checked={checked}
        onChange={onSelect}
      />
      <span
        aria-hidden="true"
        className={cn(
          "flex h-4 w-4 shrink-0 items-center justify-center rounded-full border",
          checked ? "border-accent bg-accent text-accent-contrast" : "border-line bg-surface",
        )}
      >
        {checked ? <Check className="h-3 w-3" strokeWidth={3} /> : null}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[12px] font-medium text-ink">{title}</span>
        <span className="block truncate text-[11px] text-muted">{detail}</span>
      </span>
    </label>
  );
}

function ToolSummary({ tools }: { tools: ToolStatus[] }) {
  const { t } = useTranslation();
  return (
    <div className="mt-4 flex flex-wrap gap-x-4 gap-y-1 border-t border-line pt-3 text-[10.5px] text-muted">
      {tools.map((tool) => (
        <span key={tool.tool} className="inline-flex items-center gap-1.5">
          <span
            className={cn(
              "h-1.5 w-1.5 rounded-full",
              tool.path ? "bg-accent" : "bg-warning",
            )}
            aria-hidden="true"
          />
          {tool.tool}
          {tool.version ? (
            <span className="max-w-[170px] truncate opacity-80">· {tool.version}</span>
          ) : (
            <span className="opacity-80">
              ·{" "}
              {tool.path
                ? t("media.ready", { defaultValue: "ready" })
                : t("media.missing", { defaultValue: "missing" })}
            </span>
          )}
        </span>
      ))}
    </div>
  );
}
