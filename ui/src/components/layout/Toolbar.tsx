import { Film, FolderOpen, Pause, Play, Plus, Search } from "lucide-react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Input } from "@/components/ui/Input";
import { api } from "@/lib/tauri";
import { useDownloadsStore } from "@/stores/downloads";
import { useUiStore } from "@/stores/ui";

const ACTIVE_STATES = ["probing", "downloading", "merging", "verifying", "queued"];

export function Toolbar() {
  const { t } = useTranslation();
  const selection = useUiStore((state) => state.selection);
  const search = useUiStore((state) => state.search);
  const setSearch = useUiStore((state) => state.setSearch);
  const openDialog = useUiStore((state) => state.openDialog);
  const downloads = useDownloadsStore((state) => state.downloads);
  const media = useDownloadsStore((state) => state.media);
  const run = useDownloadsStore((state) => state.run);
  const inputRef = useRef<HTMLInputElement>(null);

  const title =
    selection.kind === "library"
      ? t(`nav.${selection.filter}`)
      : selection.kind === "category"
        ? t(`categories.${selection.id}`)
        : t("nav.mainQueue");

  const activeCount =
    downloads.filter((record) => ACTIVE_STATES.includes(record.state)).length +
    media.filter((job) => job.state === "downloading" || job.state === "queued").length;
  const pausedCount = downloads.filter((record) => record.state === "paused").length;
  const total = downloads.length + media.length;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        inputRef.current?.focus();
      }
      if (event.key === "Escape" && document.activeElement === inputRef.current) {
        setSearch("");
        inputRef.current?.blur();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [setSearch]);

  return (
    <header className="shrink-0 border-b border-line bg-canvas/90 px-6 pb-3 pt-5 backdrop-blur">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="truncate text-[17px] font-semibold tracking-tight text-ink">{title}</h1>
          <p className="mt-0.5 text-[11px] text-muted">
            {t("status.downloads", { value: total })}
            {activeCount > 0 ? (
              <>
                <span className="mx-1.5 text-muted/50">·</span>
                <span className="text-accent">
                  {t("status.active", { value: activeCount, defaultValue: "{{value}} active" })}
                </span>
              </>
            ) : null}
          </p>
        </div>

        <div className="flex items-center gap-2">
          <div className="relative hidden sm:block">
            <Search
              className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted"
              aria-hidden="true"
            />
            <Input
              ref={inputRef}
              type="search"
              aria-label={t("toolbar.search")}
              placeholder={t("toolbar.search")}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              className="w-[248px] pl-8 pr-14"
            />
            <kbd className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 rounded border border-line bg-surface-2 px-1.5 py-0.5 text-[9px] font-medium text-muted">
              Ctrl K
            </kbd>
          </div>

          <IconButton
            label={t("toolbar.media", { defaultValue: "Grab media" })}
            icon={Film}
            onClick={() => openDialog("media")}
          />
          <Button variant="primary" onClick={() => openDialog("add")}>
            <Plus className="h-4 w-4" />
            {t("toolbar.add")}
          </Button>
        </div>
      </div>

      <div className="mt-3 flex items-center gap-1">
        <IconButton
          label={t("toolbar.resumeAll")}
          icon={Play}
          disabled={pausedCount === 0}
          onClick={() =>
            void run("resume-all", async () => {
              await api.resumeAll();
              await useDownloadsStore.getState().refresh();
            })
          }
        />
        <IconButton
          label={t("toolbar.pauseAll")}
          icon={Pause}
          disabled={activeCount === 0}
          onClick={() =>
            void run("pause-all", async () => {
              await api.pauseAll();
              await useDownloadsStore.getState().refresh();
            })
          }
        />
        <div className="mx-1 h-4 w-px bg-line" aria-hidden="true" />
        <IconButton
          label={t("toolbar.openFolder")}
          icon={FolderOpen}
          onClick={() =>
            void run("open-folder", async () => {
              await api.openDownloadsFolder();
            })
          }
        />
      </div>
    </header>
  );
}
