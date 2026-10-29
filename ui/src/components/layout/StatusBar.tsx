import { useTranslation } from "react-i18next";

import { useAppInfo } from "@/hooks/use-app-info";
import { formatSpeed } from "@/lib/utils";
import { useDownloadsStore } from "@/stores/downloads";

const ACTIVE_STATES = ["probing", "downloading", "merging", "verifying"];

export function StatusBar() {
  const { t } = useTranslation();
  const info = useAppInfo();
  const downloads = useDownloadsStore((state) => state.downloads);
  const speeds = useDownloadsStore((state) => state.speeds);
  const loaded = useDownloadsStore((state) => state.loaded);
  const error = useDownloadsStore((state) => state.error);

  const active = downloads.filter((record) => ACTIVE_STATES.includes(record.state));
  const speed = active.reduce((total, record) => total + (speeds[record.id] ?? 0), 0);
  const online = loaded && !error;
  const label = info?.platform === "browser" ? t("status.browserMode") : t("status.engineReady");

  return (
    <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-line bg-surface px-4 text-[11px] text-muted">
      <span className="inline-flex items-center gap-1.5">
        <span
          className={`h-1.5 w-1.5 rounded-full ${online ? "bg-accent" : "bg-warning"}`}
          aria-hidden="true"
        />
        {online ? label : t("status.engineOffline")}
      </span>
      <span className="h-3 w-px bg-line" aria-hidden="true" />
      <span>
        {t("status.activeCount", {
          value: active.length,
          defaultValue: "{{value}} active",
        })}
      </span>
      <span className="tabular-nums">{formatSpeed(speed)}</span>
      <span className="ml-auto tabular-nums">
        {info ? `${info.name} ${info.version}` : "Rilldown"}
      </span>
    </footer>
  );
}
