import { beforeEach, describe, expect, it } from "vitest";

import type { DownloadRecord, MediaJob } from "@/lib/tauri";
import { useDownloadsStore } from "@/stores/downloads";

function record(overrides: Partial<DownloadRecord> = {}): DownloadRecord {
  return {
    id: "11111111-1111-1111-1111-111111111111",
    url: "https://example.com/file.zip",
    final_url: null,
    save_dir: "/tmp/downloads",
    filename: "file.zip",
    custom_filename: false,
    state: "downloading",
    total_bytes: 1000,
    downloaded_bytes: 100,
    resumable: true,
    etag: null,
    last_modified: null,
    content_type: null,
    error: null,
    checksum: null,
    checksum_actual: null,
    created_at: "2026-10-29T10:00:00Z",
    updated_at: "2026-10-29T10:00:00Z",
    completed_at: null,
    queue_id: "main",
    category: "compressed",
    connections: 4,
    speed_limit_bps: null,
    headers: {},
    cookies: null,
    referrer: null,
    user_agent: null,
    proxy: null,
    mirrors: [],
    ...overrides,
  };
}

function mediaJob(overrides: Partial<MediaJob> = {}): MediaJob {
  return {
    id: "media-1",
    url: "https://example.com/watch",
    title: "Example",
    formatId: null,
    state: "downloading",
    percent: 10,
    downloadedBytes: 100,
    totalBytes: 1000,
    speedBps: 50,
    etaSeconds: 18,
    stage: "downloading",
    error: null,
    path: null,
    createdAt: 1,
    ...overrides,
  };
}

beforeEach(() => {
  useDownloadsStore.setState({
    downloads: [record()],
    media: [],
    speeds: {},
    loaded: true,
    error: null,
    busy: {},
    clipboardUrl: null,
  });
});

describe("downloads store", () => {
  it("applies progress snapshots to the matching record", () => {
    useDownloadsStore.getState().applyEvent({
      type: "progress",
      progress: {
        id: "11111111-1111-1111-1111-111111111111",
        state: "downloading",
        downloaded_bytes: 640,
        total_bytes: 1000,
        speed_bps: 2048,
        eta_seconds: 1,
        connections: [],
      },
    });

    const state = useDownloadsStore.getState();
    expect(state.downloads[0]?.downloaded_bytes).toBe(640);
    expect(state.speeds["11111111-1111-1111-1111-111111111111"]).toBe(2048);
  });

  it("applies state changes and failure messages", () => {
    useDownloadsStore.getState().applyEvent({
      type: "state_changed",
      id: "11111111-1111-1111-1111-111111111111",
      state: "failed",
      error: "HTTP 404",
    });

    const [updated] = useDownloadsStore.getState().downloads;
    expect(updated?.state).toBe("failed");
    expect(updated?.error).toBe("HTTP 404");
  });

  it("inserts and updates media jobs without duplicates", () => {
    const store = useDownloadsStore.getState();
    store.upsertMedia(mediaJob());
    store.upsertMedia(mediaJob({ percent: 55 }));

    const { media } = useDownloadsStore.getState();
    expect(media).toHaveLength(1);
    expect(media[0]?.percent).toBe(55);
  });

  it("records mutation errors and clears busy flags", async () => {
    await useDownloadsStore.getState().run("test", async () => {
      throw new Error("engine said no");
    });

    const state = useDownloadsStore.getState();
    expect(state.error).toBe("engine said no");
    expect(state.busy.test).toBeUndefined();
  });
});
