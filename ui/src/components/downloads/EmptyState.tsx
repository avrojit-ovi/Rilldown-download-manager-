import { Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/Button";
import { useUiStore } from "@/stores/ui";

export type EmptyKind = "all" | "downloading" | "queued" | "completed" | "failed" | "search" | "category" | "queue";

const COPY: Record<EmptyKind, { title: string; body: string }> = {
  all: { title: "empty.allTitle", body: "empty.allBody" },
  downloading: { title: "empty.downloadingTitle", body: "empty.downloadingBody" },
  queued: { title: "empty.queuedTitle", body: "empty.queuedBody" },
  completed: { title: "empty.completedTitle", body: "empty.completedBody" },
  failed: { title: "empty.failedTitle", body: "empty.failedBody" },
  search: { title: "empty.searchTitle", body: "empty.searchBody" },
  category: { title: "empty.categoryTitle", body: "empty.categoryBody" },
  queue: { title: "empty.queueTitle", body: "empty.queueBody" },
};

export function EmptyState({ kind, label }: { kind: EmptyKind; label?: string }) {
  const { t } = useTranslation();
  const setSearch = useUiStore((state) => state.setSearch);
  const openDialog = useUiStore((state) => state.openDialog);
  const copy = COPY[kind];

  return (
    <div className="flex h-full min-h-[420px] flex-col items-center justify-center px-8 pb-16 pt-10 text-center">
      <StreamIllustration />
      {label ? (
        <div className="mb-3 inline-flex max-w-[280px] items-center gap-1.5 rounded-full border border-line bg-surface px-2.5 py-1 text-[11px] text-muted">
          <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
          <span className="truncate">{label}</span>
        </div>
      ) : null}
      <h2 className="text-[19px] font-semibold tracking-tight text-ink">{t(copy.title)}</h2>
      <p className="mt-2 max-w-[400px] text-[13px] leading-6 text-muted">{t(copy.body)}</p>

      {kind === "search" ? (
        <Button className="mt-6" variant="secondary" onClick={() => setSearch("")}>
          {t("empty.clearSearch")}
        </Button>
      ) : (
        <>
          <Button className="mt-6" variant="primary" onClick={() => openDialog("add")}>
            <Plus className="h-4 w-4" />
            {t("empty.cta")}
          </Button>
          <p className="mt-2.5 text-[11px] text-muted/80">{t("status.phaseNote")}</p>
        </>
      )}
    </div>
  );
}

function StreamIllustration() {
  return (
    <div className="empty-glow relative mb-7 flex h-[152px] w-[212px] items-end justify-center overflow-hidden rounded-[28px] border border-line/80 pb-9 shadow-soft">
      <div className="absolute top-7 h-24 w-24 rounded-full border border-dashed border-accent/20" />
      <div className="relative flex items-end gap-2.5">
        <span className="stream-bar h-[52px] w-2.5 rounded-full bg-accent/45" />
        <span className="stream-bar h-[82px] w-2.5 rounded-full bg-accent" />
        <span className="stream-bar h-[64px] w-2.5 rounded-full bg-accent/70" />
      </div>
      <div className="absolute bottom-6 h-[7px] w-[88px] rounded-full bg-accent/80" />
    </div>
  );
}
