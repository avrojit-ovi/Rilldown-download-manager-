import {
  AlertCircle,
  AppWindow,
  ArrowDownToLine,
  CheckCircle2,
  Clock3,
  FileArchive,
  FileText,
  Inbox,
  Languages,
  Layers,
  Monitor,
  Moon,
  Music,
  Package,
  Settings2,
  Sun,
  Video,
  type LucideIcon,
} from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { LogoMark, Wordmark } from "@/components/brand/Logo";
import { IconButton } from "@/components/ui/IconButton";
import { cn } from "@/lib/utils";
import { useDownloadsStore } from "@/stores/downloads";
import {
  useUiStore,
  type CategoryId,
  type LibraryFilter,
  type Selection,
  type ThemeMode,
} from "@/stores/ui";

const LIBRARY: { filter: LibraryFilter; labelKey: string; icon: LucideIcon }[] = [
  { filter: "all", labelKey: "nav.all", icon: Inbox },
  { filter: "downloading", labelKey: "nav.downloading", icon: ArrowDownToLine },
  { filter: "queued", labelKey: "nav.queued", icon: Clock3 },
  { filter: "completed", labelKey: "nav.completed", icon: CheckCircle2 },
  { filter: "failed", labelKey: "nav.failed", icon: AlertCircle },
];

const CATEGORIES: { id: CategoryId; labelKey: string; icon: LucideIcon }[] = [
  { id: "compressed", labelKey: "categories.compressed", icon: FileArchive },
  { id: "documents", labelKey: "categories.documents", icon: FileText },
  { id: "music", labelKey: "categories.music", icon: Music },
  { id: "video", labelKey: "categories.video", icon: Video },
  { id: "programs", labelKey: "categories.programs", icon: AppWindow },
  { id: "other", labelKey: "categories.other", icon: Package },
];

const THEME_ORDER: ThemeMode[] = ["system", "light", "dark"];

const ACTIVE_STATES = ["probing", "downloading", "merging", "verifying"];

export function Sidebar() {
  const { t } = useTranslation();
  const selection = useUiStore((state) => state.selection);
  const select = useUiStore((state) => state.select);
  const theme = useUiStore((state) => state.theme);
  const setTheme = useUiStore((state) => state.setTheme);
  const language = useUiStore((state) => state.language);
  const setLanguage = useUiStore((state) => state.setLanguage);
  const openDialog = useUiStore((state) => state.openDialog);
  const downloads = useDownloadsStore((state) => state.downloads);
  const media = useDownloadsStore((state) => state.media);
  const queues = useDownloadsStore((state) => state.queues);

  const themeIcon = theme === "system" ? Monitor : theme === "dark" ? Moon : Sun;
  const nextTheme = THEME_ORDER[(THEME_ORDER.indexOf(theme) + 1) % THEME_ORDER.length];

  const libraryCounts: Record<LibraryFilter, number> = {
    all: downloads.length + media.length,
    downloading:
      downloads.filter((record) => ACTIVE_STATES.includes(record.state)).length +
      media.filter((job) => job.state === "downloading" || job.state === "queued").length,
    queued: downloads.filter((record) => record.state === "queued").length,
    completed:
      downloads.filter((record) => record.state === "completed").length +
      media.filter((job) => job.state === "completed").length,
    failed:
      downloads.filter((record) => record.state === "failed").length +
      media.filter((job) => job.state === "failed" || job.state === "cancelled").length,
  };

  const categoryCounts = CATEGORIES.reduce<Record<string, number>>((counts, category) => {
    counts[category.id] = downloads.filter(
      (record) => record.category === category.id,
    ).length;
    return counts;
  }, {});

  const queueList =
    queues.length > 0 ? queues : [{ id: "main", name: t("nav.mainQueue"), max_concurrent: 0 }];

  const queueCount = (id: string) =>
    downloads.filter((record) => (record.queue_id ?? "main") === id).length;

  return (
    <aside className="sidebar-wash flex w-[252px] shrink-0 flex-col border-r border-line">
      <div className="flex h-16 items-center gap-3 px-5">
        <LogoMark />
        <Wordmark />
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto px-3 pb-4" aria-label={t("nav.library")}>
        <SectionLabel>{t("nav.library")}</SectionLabel>
        <div className="space-y-0.5">
          {LIBRARY.map((item) => {
            const active = selection.kind === "library" && selection.filter === item.filter;
            return (
              <NavButton
                key={item.filter}
                icon={item.icon}
                label={t(item.labelKey)}
                active={active}
                count={libraryCounts[item.filter]}
                onClick={() => select({ kind: "library", filter: item.filter } satisfies Selection)}
              />
            );
          })}
        </div>

        <SectionLabel className="mt-5">{t("nav.categories")}</SectionLabel>
        <div className="space-y-0.5">
          {CATEGORIES.map((item) => {
            const active = selection.kind === "category" && selection.id === item.id;
            return (
              <NavButton
                key={item.id}
                icon={item.icon}
                label={t(item.labelKey)}
                active={active}
                count={categoryCounts[item.id] ?? 0}
                onClick={() => select({ kind: "category", id: item.id } satisfies Selection)}
              />
            );
          })}
        </div>

        <SectionLabel className="mt-5">{t("nav.queues")}</SectionLabel>
        <div className="space-y-0.5">
          {queueList.map((queue) => (
            <NavButton
              key={queue.id}
              icon={Layers}
              label={queue.name}
              active={selection.kind === "queue" && selection.id === queue.id}
              count={queueCount(queue.id)}
              onClick={() => select({ kind: "queue", id: queue.id } satisfies Selection)}
            />
          ))}
        </div>
      </nav>

      <div className="flex items-center gap-1 border-t border-line px-3 py-2.5">
        <IconButton
          label={`${t("theme.label")}: ${t(`theme.${theme}`)}`}
          icon={themeIcon}
          onClick={() => setTheme(nextTheme)}
        />
        <IconButton
          label={`${t("language.label")}: ${t(`language.${language}`)}`}
          icon={Languages}
          onClick={() => setLanguage(language === "en" ? "bn" : "en")}
        />
        <span className="ml-1 text-[11px] font-medium uppercase tracking-wider text-muted">
          {language === "en" ? "EN" : "বাং"}
        </span>
        <IconButton
          className="ml-auto"
          label={t("toolbar.settings")}
          icon={Settings2}
          onClick={() => openDialog("settings")}
        />
      </div>
    </aside>
  );
}

function SectionLabel({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <p
      className={cn(
        "px-2 pb-1.5 pt-2 text-[9.5px] font-semibold uppercase tracking-[0.16em] text-muted/80",
        className,
      )}
    >
      {children}
    </p>
  );
}

function NavButton({
  icon: Icon,
  label,
  active,
  count,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  active: boolean;
  count: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={cn(
        "group flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px] transition-colors",
        active
          ? "bg-accent-soft font-medium text-accent"
          : "text-muted hover:bg-surface-2 hover:text-ink",
      )}
    >
      <Icon className="h-4 w-4 shrink-0" strokeWidth={1.85} aria-hidden="true" />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {count > 0 ? (
        <span
          aria-hidden="true"
          className="rounded-full bg-surface px-1.5 py-0.5 text-[10px] font-medium tabular-nums text-muted"
        >
          {count}
        </span>
      ) : null}
    </button>
  );
}
