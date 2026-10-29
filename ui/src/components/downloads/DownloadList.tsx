import { useTranslation } from "react-i18next";

import { DownloadRow } from "@/components/downloads/DownloadRow";
import { EmptyState } from "@/components/downloads/EmptyState";
import { MediaRow } from "@/components/downloads/MediaRow";
import type { DownloadRecord, DownloadState, MediaJob, MediaJobState } from "@/lib/tauri";
import { useDownloadsStore } from "@/stores/downloads";
import { useUiStore } from "@/stores/ui";

type Row =
  | { kind: "download"; record: DownloadRecord; created: number }
  | { kind: "media"; job: MediaJob; created: number };

const ACTIVE_STATES: DownloadState[] = [
  "probing",
  "downloading",
  "merging",
  "verifying",
];

const ACTIVE_MEDIA: MediaJobState[] = ["queued", "downloading"];

function createdOf(value: string): number {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? 0 : parsed;
}

function rankOf(row: Row): number {
  if (row.kind === "download") {
    const { state } = row.record;
    if (state === "queued" || ACTIVE_STATES.includes(state)) {
      return 0;
    }
    if (state === "paused" || state === "failed") {
      return 1;
    }
    return 2;
  }
  const { state } = row.job;
  if (ACTIVE_MEDIA.includes(state)) {
    return 0;
  }
  if (state === "failed") {
    return 1;
  }
  return 2;
}

function matchesSearch(row: Row, needle: string): boolean {
  if (!needle) {
    return true;
  }
  const fields =
    row.kind === "download"
      ? [row.record.filename, row.record.url]
      : [row.job.title, row.job.url, row.job.path];
  return fields.some((field) => field?.toLowerCase().includes(needle));
}

function matchesLibrary(row: Row, filter: string): boolean {
  if (row.kind === "download") {
    const { state } = row.record;
    switch (filter) {
      case "downloading":
        return ACTIVE_STATES.includes(state);
      case "queued":
        return state === "queued";
      case "completed":
        return state === "completed";
      case "failed":
        return state === "failed";
      default:
        return true;
    }
  }
  const { state } = row.job;
  switch (filter) {
    case "downloading":
      return ACTIVE_MEDIA.includes(state);
    case "queued":
      return false;
    case "completed":
      return state === "completed";
    case "failed":
      return state === "failed" || state === "cancelled";
    default:
      return true;
  }
}

export function DownloadList() {
  const { t } = useTranslation();
  const downloads = useDownloadsStore((state) => state.downloads);
  const media = useDownloadsStore((state) => state.media);
  const queues = useDownloadsStore((state) => state.queues);
  const selection = useUiStore((state) => state.selection);
  const search = useUiStore((state) => state.search);
  const needle = search.trim().toLowerCase();

  const allRows: Row[] = [
    ...downloads.map<Row>((record) => ({
      kind: "download",
      record,
      created: createdOf(record.created_at),
    })),
    ...media.map<Row>((job) => ({
      kind: "media",
      job,
      created: job.createdAt,
    })),
  ];

  let rows: Row[];
  if (selection.kind === "library") {
    rows = allRows.filter((row) => matchesLibrary(row, selection.filter));
  } else if (selection.kind === "category") {
    rows = allRows.filter(
      (row) => row.kind === "download" && row.record.category === selection.id,
    );
  } else {
    rows = allRows.filter(
      (row) =>
        row.kind === "download" &&
        (row.record.queue_id ?? "main") === selection.id,
    );
  }
  rows = rows.filter((row) => matchesSearch(row, needle));
  rows.sort((left, right) => {
    const rank = rankOf(left) - rankOf(right);
    return rank !== 0 ? rank : right.created - left.created;
  });

  if (rows.length === 0) {
    if (needle) {
      return <EmptyState kind="search" label={search.trim()} />;
    }
    if (selection.kind === "category") {
      return <EmptyState kind="category" label={t(`categories.${selection.id}`)} />;
    }
    if (selection.kind === "queue") {
      const queue = queues.find((candidate) => candidate.id === selection.id);
      return <EmptyState kind="queue" label={queue?.name ?? t("nav.mainQueue")} />;
    }
    return <EmptyState kind={selection.filter} />;
  }

  return (
    <div className="px-4 py-4 sm:px-6">
      <div className="mx-auto flex max-w-[980px] flex-col gap-2">
        {rows.map((row) =>
          row.kind === "download" ? (
            <DownloadRow key={`download:${row.record.id}`} record={row.record} />
          ) : (
            <MediaRow key={`media:${row.job.id}`} job={row.job} />
          ),
        )}
      </div>
    </div>
  );
}
