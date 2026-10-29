import { Film, Trash2, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { IconButton } from "@/components/ui/IconButton";
import { Progress } from "@/components/ui/Progress";
import { api, type MediaJob } from "@/lib/tauri";
import { cn, formatBytes, formatSpeed } from "@/lib/utils";
import { useDownloadsStore } from "@/stores/downloads";

const STAGE_LABEL: Record<string, { key: string; fallback: string }> = {
  starting: { key: "media.stageStarting", fallback: "Preparing" },
  downloading: { key: "media.stageDownloading", fallback: "Downloading" },
  merging: { key: "media.stageMerging", fallback: "Merging" },
  finalizing: { key: "media.stageFinalizing", fallback: "Finalizing" },
  completed: { key: "media.stageCompleted", fallback: "Completed" },
  failed: { key: "media.stageFailed", fallback: "Failed" },
  cancelled: { key: "media.stageCancelled", fallback: "Cancelled" },
};

function hostOf(value: string): string {
  try {
    return new URL(value).hostname;
  } catch {
    return value;
  }
}

function formatEta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) {
    return "";
  }
  if (seconds < 60) {
    return `${Math.max(1, Math.round(seconds))}s`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ${Math.round(seconds % 60)}s`;
  }
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function MediaRow({ job }: { job: MediaJob }) {
  const { t } = useTranslation();
  const busy = useDownloadsStore((state) => state.busy);
  const run = useDownloadsStore((state) => state.run);
  const removeMedia = useDownloadsStore((state) => state.removeMedia);

  const running = job.state === "downloading" || job.state === "queued";
  const terminal = !running;
  const fraction = job.percent !== null ? Math.min(job.percent / 100, 1) : 0;
  const indeterminate = running && job.percent === null && !job.totalBytes;
  const stage = STAGE_LABEL[job.stage] ?? {
    key: `media.stage.${job.stage}`,
    fallback: job.stage,
  };
  const busyKey = Boolean(busy[`media:${job.id}`]);
  const title = job.title && job.title !== job.url ? job.title : hostOf(job.url);

  const cancel = () =>
    run(`media:${job.id}`, async () => {
      await api.mediaCancel(job.id);
    });

  const remove = () =>
    run(`media:${job.id}`, async () => {
      await api.mediaRemove(job.id);
      removeMedia(job.id);
    });

  return (
    <article className="group rounded-xl border border-line bg-surface px-3.5 py-3 shadow-soft transition-colors hover:border-line/80">
      <div className="flex items-start gap-3">
        <span className="mt-0.5 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
          <Film className="h-4 w-4" strokeWidth={1.9} aria-hidden="true" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <h3 className="min-w-0 max-w-full truncate text-[13.5px] font-medium text-ink">
              {title}
            </h3>
            <span
              className={cn(
                "inline-flex shrink-0 items-center rounded-full px-1.5 py-0.5 text-[9.5px] font-semibold uppercase tracking-[0.08em]",
                job.state === "completed" && "bg-accent-soft text-accent",
                job.state === "downloading" && "bg-accent-soft text-accent",
                job.state === "queued" && "bg-surface-2 text-muted",
                job.state === "failed" && "bg-danger/10 text-danger",
                job.state === "cancelled" && "bg-surface-2 text-muted",
              )}
            >
              {t("media.badge", { defaultValue: "Media" })}
            </span>
            <span className="text-[11px] text-muted">
              {t(stage.key, { defaultValue: stage.fallback })}
            </span>
            {job.percent !== null ? (
              <span className="text-[11px] tabular-nums text-muted">
                {Math.round(job.percent)}%
              </span>
            ) : null}
          </div>
          <p className="mt-1 truncate text-[11.5px] text-muted">
            {hostOf(job.url)}
            {job.path ? (
              <>
                <span className="mx-1.5 text-muted/50">·</span>
                <span className="text-muted/80">{job.path}</span>
              </>
            ) : null}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          {running ? (
            <IconButton
              label={t("media.cancel", { defaultValue: "Cancel media grab" })}
              icon={X}
              onClick={() => void cancel()}
              disabled={busyKey}
            />
          ) : null}
          {terminal ? (
            <IconButton
              label={t("media.remove", { defaultValue: "Remove media grab" })}
              icon={Trash2}
              onClick={() => void remove()}
              disabled={busyKey}
            />
          ) : null}
        </div>
      </div>

      {running || job.state === "completed" ? (
        <Progress
          className="mt-3"
          value={fraction}
          indeterminate={indeterminate}
          tone={job.state === "completed" ? "muted" : "accent"}
          label={`${title} ${t("media.grabProgress", { defaultValue: "media grab progress" })}`}
        />
      ) : (
        <Progress
          className="mt-3"
          value={fraction}
          tone="danger"
          label={`${title} ${t("media.grabProgress", { defaultValue: "media grab progress" })}`}
        />
      )}

      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
        <span className="tabular-nums">
          {job.totalBytes
            ? t("downloads.sizeOf", {
                defaultValue: "{{done}} of {{total}}",
                done: formatBytes(job.downloadedBytes),
                total: formatBytes(job.totalBytes),
              })
            : formatBytes(job.downloadedBytes)}
        </span>
        {job.state === "downloading" && job.speedBps > 0 ? (
          <span className="tabular-nums text-accent">{formatSpeed(job.speedBps)}</span>
        ) : null}
        {job.state === "downloading" && job.etaSeconds !== null ? (
          <span className="tabular-nums">{formatEta(job.etaSeconds)}</span>
        ) : null}
      </div>

      {job.error ? (
        <p className="mt-2 line-clamp-2 rounded-lg bg-danger/5 px-2 py-1.5 text-[11px] leading-5 text-danger">
          {job.error}
        </p>
      ) : null}
    </article>
  );
}
