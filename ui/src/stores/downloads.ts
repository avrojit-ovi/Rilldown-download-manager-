import { create } from "zustand";

import {
  api,
  isTauri,
  type AppSettings,
  type BrowserHostStatus,
  type DownloadRecord,
  type EngineEvent,
  type MediaJob,
  type Queue,
  type ToolStatus,
} from "@/lib/tauri";

interface DownloadsState {
  downloads: DownloadRecord[];
  media: MediaJob[];
  queues: Queue[];
  settings: AppSettings | null;
  tools: ToolStatus[];
  host: BrowserHostStatus | null;
  speeds: Record<string, number>;
  loaded: boolean;
  error: string | null;
  busy: Record<string, boolean>;
  clipboardUrl: string | null;

  refresh: () => Promise<void>;
  refreshMedia: () => Promise<void>;
  refreshQueues: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshTools: () => Promise<void>;
  refreshHost: () => Promise<void>;
  refreshAll: () => Promise<void>;
  applyEvent: (event: EngineEvent) => void;
  upsertMedia: (job: MediaJob) => void;
  removeMedia: (id: string) => void;
  setSettings: (settings: AppSettings) => void;
  setQueues: (queues: Queue[]) => void;
  setTools: (tools: ToolStatus[]) => void;
  setHost: (host: BrowserHostStatus | null) => void;
  setClipboardUrl: (url: string | null) => void;
  clearError: () => void;
  run: (key: string, action: () => Promise<void>) => Promise<void>;
}

let refreshTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleRefresh(get: () => DownloadsState) {
  if (refreshTimer) {
    return;
  }
  refreshTimer = setTimeout(() => {
    refreshTimer = null;
    void get().refresh();
  }, 180);
}

export const useDownloadsStore = create<DownloadsState>((set, get) => ({
  downloads: [],
  media: [],
  queues: [],
  settings: null,
  tools: [],
  host: null,
  speeds: {},
  loaded: false,
  error: null,
  busy: {},
  clipboardUrl: null,

  refresh: async () => {
    if (!isTauri()) {
      set({ loaded: true });
      return;
    }
    try {
      const downloads = await api.listDownloads();
      set({ downloads, loaded: true, error: null });
    } catch (error) {
      set({ error: messageOf(error) });
    }
  },

  refreshMedia: async () => {
    if (!isTauri()) {
      return;
    }
    try {
      const media = await api.mediaJobs();
      set({ media });
    } catch (error) {
      set({ error: messageOf(error) });
    }
  },

  refreshQueues: async () => {
    if (!isTauri()) {
      return;
    }
    try {
      set({ queues: await api.listQueues() });
    } catch (error) {
      set({ error: messageOf(error) });
    }
  },

  refreshSettings: async () => {
    if (!isTauri()) {
      return;
    }
    try {
      set({ settings: await api.getSettings() });
    } catch (error) {
      set({ error: messageOf(error) });
    }
  },

  refreshTools: async () => {
    if (!isTauri()) {
      return;
    }
    try {
      set({ tools: await api.toolsStatus() });
    } catch (error) {
      set({ error: messageOf(error) });
    }
  },

  refreshHost: async () => {
    if (!isTauri()) {
      return;
    }
    try {
      set({ host: await api.browserHostStatus() });
    } catch (error) {
      set({ error: messageOf(error) });
    }
  },

  refreshAll: async () => {
    await Promise.all([
      get().refresh(),
      get().refreshMedia(),
      get().refreshQueues(),
      get().refreshSettings(),
      get().refreshTools(),
      get().refreshHost(),
    ]);
  },

  applyEvent: (event) => {
    if (event.type === "progress") {
      const { progress } = event;
      set((state) => ({
        speeds: { ...state.speeds, [progress.id]: progress.speed_bps },
        downloads: state.downloads.map((record) =>
          record.id === progress.id
            ? {
                ...record,
                state: progress.state,
                downloaded_bytes: progress.downloaded_bytes,
                total_bytes: progress.total_bytes,
              }
            : record,
        ),
      }));
      return;
    }
    if (event.type === "state_changed") {
      set((state) => ({
        downloads: state.downloads.map((record) =>
          record.id === event.id
            ? { ...record, state: event.state, error: event.error }
            : record,
        ),
      }));
      scheduleRefresh(get);
      return;
    }
    if (event.type === "completed") {
      scheduleRefresh(get);
    }
  },

  upsertMedia: (job) =>
    set((state) => {
      const exists = state.media.some((candidate) => candidate.id === job.id);
      const media = exists
        ? state.media.map((candidate) => (candidate.id === job.id ? job : candidate))
        : [job, ...state.media];
      return { media };
    }),

  removeMedia: (id) =>
    set((state) => ({ media: state.media.filter((job) => job.id !== id) })),

  setSettings: (settings) => set({ settings }),
  setQueues: (queues) => set({ queues }),
  setTools: (tools) => set({ tools }),
  setHost: (host) => set({ host }),
  setClipboardUrl: (clipboardUrl) => set({ clipboardUrl }),
  clearError: () => set({ error: null }),

  run: async (key, action) => {
    set((state) => ({ busy: { ...state.busy, [key]: true } }));
    try {
      await action();
      set({ error: null });
    } catch (error) {
      set({ error: messageOf(error) });
    } finally {
      set((state) => {
        const busy = { ...state.busy };
        delete busy[key];
        return { busy };
      });
    }
  },
}));

function messageOf(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
