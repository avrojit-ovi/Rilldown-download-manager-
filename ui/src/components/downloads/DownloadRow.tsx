import {
  ExternalLink,
  FolderOpen,
  Pause,
  Play,
  RotateCcw,
  Trash2,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { IconButton } from "@/components/ui/IconButton";
import { Progress } from "@/components/ui/Progress";
import { api, type DownloadRecord, type DownloadState } from "@/lib/tauri";
import { cn, formatBytes, formatSpeed } from "@/lib/utils";
import { useDownloadsStore } from "@/stores/downloads";

const ACTIVE: DownloadState[] = ["probing", "downloading", "merging", "verifying"];

const STATE_LABEL: Record<DownloadState, { key: string; fallback: string }> = {
  queued: { key: "states.queued", fallback: "Queued" },
  probing: { key: "states.probing", fallback: "Checking" },
  downloading: { key: "states.downloading", fallback: "Downloading" },
  paused: { key: "states.paused", fallback: "Paused" },
  merging: { key: "states.merging", fallback: "Merging" },
  verifying: { key: "states.verifying", fallback: "Verifying" },
  completed: { key: "states.completed", fallback: "Completed" },
  failed: { key: "states.failed", fallback: "Failed" },
  cancelled: { key: "states.cancelled", fallback: "Cancelled" },
};

function hostOf(value: string): string {
  try {
    return new URL(value).hostname;
  } catch {
    return value;
  }
}

function formatEta(seconds: number | null, fallback: string): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) {
    return fallback;
  }
  if (seconds < 60) {
    return `${Math.max(1, Math.round(seconds))}s`;
  }
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.round(seconds % 60);
  if (minutes < 60) {
    return `${minutes}m ${remainder}s`;
  }
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export function DownloadRow({ record }: { record: DownloadRecord }) {
  const { t } = useTranslation();
  const speed = useDownloadsStore((state) => state.speeds[record.id] ?? 0);
  const busy = useDownloadsStore((state) => state.busy);
  const run = useDownloadsStore((state) => state.run);
  const refresh = useDownloadsStore((state) => state.refresh);

  const active = ACTIVE.includes(record.state);
  const completed = record.state === "completed";
  const failed = record.state === "failed";
  const paused = record.state === "paused";
  const cancelled = record.state === "cancelled";
  const queued = record.state === "queued";

  const fraction =
    completed
      ? 1
      : record.total_bytes && record.total_bytes > 0
        ? Math.min(record.downloaded_bytes / record.total_bytes, 1)
        : 0;
  const indeterminate = active && record.total_bytes === null;
  const percent = record.total_bytes
    ? `${Math.round(fraction * 100)}%`
    : completed
      ? "100%"
      : "";

  const busyKey = (action: string) => Boolean(busy[`${record.id}:${action}`]);
  const stateLabel = t(STATE_LABEL[record.state].key, {
    defaultValue: STATE_LABEL[record.state].fallback,
  });

  const pause = () =>
    run(`${record.id}:pause`, async () => {
      await api.pauseDownload(record.id);
      await refresh();
    });
  const resume = () =>
    run(`${record.id}:resume`, async () => {
      await api.resumeDownload(record.id);
      await refresh();
    });
  const cancel = () =>
    run(`${record.id}:cancel`, async () => {
      await api.cancelDownload(record.id);
      await refresh();
    });
  const remove = () =>
    run(`${record.id}:remove`, async () => {
      await api.removeDownload(record.id, false);
      await refresh();
    });
  const open = () =>
    run(`${record.id}:open`, async () => {
      await api.openDownload(record.id);
    });
  const reveal = () =>
    run(`${record.id}:reveal`, async () => {
      await api.revealDownload(record.id);
    });

  const actionName = (label: string) =>
    t("downloads.namedAction", { defaultValue: `${label} {{name}}`, name: record.filename });

  return (
    <article className="group rounded-xl border border-line bg-surface px-3.5 py-3 shadow-soft transition-colors hover:border-line/80">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <h3 className="min-w-0 max-w-full truncate text-[13.5px] font-medium text-ink">
              {record.filename}
            </h3>
            <StateChip state={record.state} label={stateLabel} />
            {percent ? (
              <span className="text-[11px] tabular-nums text-muted">{percent}</span>
            ) : null}
          </div>
          <p className="mt-1 truncate text-[11.5px] text-muted">
            {hostOf(record.url)}
            {record.save_dir ? (
              <>
                <span className="mx-1.5 text-muted/50">·</span>
                <span className="text-muted/80">{record.save_dir}</span>
              </>
            ) : null}
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-0.5 opacity-90">
          {active || queued ? (
            <>
              <IconButton
                label={actionName(t("downloads.pause", { defaultValue: "Pause" }))}
                icon={Pause}
                onClick={() => void pause()}
                disabled={busyKey("pause")}
              />
              <IconButton
                label={actionName(t("downloads.cancel", { defaultValue: "Cancel" }))}
                icon={X}
                onClick={() => void cancel()}
                disabled={busyKey("cancel")}
              />
            </>
          ) : null}
          {paused || failed || cancelled ? (
            <>
              <IconButton
                label={actionName(
                  failed
                    ? t("downloads.retry", { defaultValue: "Retry" })
                    : t("downloads.resume", { defaultValue: "Resume" }),
                )}
                icon={failed || cancelled ? RotateCcw : Play}
                onClick={() => void resume()}
                disabled={busyKey("resume")}
              />
              <IconButton
                label={actionName(t("downloads.remove", { defaultValue: "Remove from list" }))}
                icon={Trash2}
                onClick={() => void remove()}
                disabled={busyKey("remove")}
              />
            </>
          ) : null}
          {completed ? (
            <>
              <IconButton
                label={actionName(t("downloads.open", { defaultValue: "Open" }))}
                icon={ExternalLink}
                onClick={() => void open()}
                disabled={busyKey("open")}
              />
              <IconButton
                label={actionName(
                  t("downloads.reveal", { defaultValue: "Reveal in folder" }),
                )}
                icon={FolderOpen}
                onClick={() => void reveal()}
                disabled={busyKey("reveal")}
              />
              <IconButton
                label={actionName(t("downloads.remove", { defaultValue: "Remove from list" }))}
                icon={Trash2}
                onClick={() => void remove()}
                disabled={busyKey("remove")}
              />
            </>
          ) : null}
        </div>
      </div>

      {active || paused || queued || completed ? (
        <Progress
          className="mt-3"
          value={fraction}
          indeterminate={indeterminate}
          tone={completed ? "muted" : "accent"}
          label={`${record.filename} ${t("downloads.progress", { defaultValue: "progress" })}`}
        />
      ) : failed ? (
        <Progress
          className="mt-3"
          value={fraction}
          tone="danger"
          label={`${record.filename} ${t("downloads.progress", { defaultValue: "progress" })}`}
        />
      ) : null}

      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
        <span className="tabular-nums">
          {record.total_bytes
            ? t("downloads.sizeOf", {
                defaultValue: "{{done}} of {{total}}",
                done: formatBytes(record.downloaded_bytes),
                total: formatBytes(record.total_bytes),
              })
            : formatBytes(record.downloaded_bytes)}
        </span>
        {active && speed > 0 ? (
          <span className="tabular-nums text-accent">{formatSpeed(speed)}</span>
        ) : null}
        {active && record.total_bytes !== null && speed > 0 ? (
          <span className="tabular-nums">
            {formatEta(
              Math.max(0, (record.total_bytes - record.downloaded_bytes) / speed),
              t("downloads.etaUnknown", { defaultValue: "—" }),
            )}
          </span>
        ) : null}
        {record.connections > 0 && (active || queued) ? (
          <span>
            {t("downloads.connections", {
              defaultValue: "{{count}} connections",
              count: record.connections,
            })}
          </span>
        ) : null}
        {active && !indeterminate && !record.resumable ? (
          <span className="text-warning">
            {t("downloads.singleStream", { defaultValue: "single stream" })}
          </span>
        ) : null}
      </div>

      {failed && record.error ? (
        <p className="mt-2 line-clamp-2 rounded-lg bg-danger/5 px-2 py-1.5 text-[11px] leading-5 text-danger">
          {record.error}
        </p>
      ) : null}
    </article>
  );
}

function StateChip({ state, label }: { state: DownloadState; label: string }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center rounded-full px-1.5 py-0.5 text-[9.5px] font-semibold uppercase tracking-[0.08em]",
        state === "completed" && "bg-accent-soft text-accent",
        (state === "downloading" || state === "probing" || state === "merging" || state === "verifying") &&
          "bg-accent-soft text-accent",
        state === "queued" && "bg-surface-2 text-muted",
        state === "paused" && "bg-surface-2 text-muted",
        state === "failed" && "bg-danger/10 text-danger",
        state === "cancelled" && "bg-surface-2 text-muted",
      )}
    >
      {label}
    </span>
  );
}
