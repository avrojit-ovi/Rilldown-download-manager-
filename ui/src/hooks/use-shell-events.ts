import { useEffect } from "react";

import {
  api,
  onClipboardUrl,
  onEngineEvent,
  onMediaJob,
  onOpenSettings,
} from "@/lib/tauri";
import { useDownloadsStore } from "@/stores/downloads";
import { useUiStore } from "@/stores/ui";

/**
 * Wire the shell's event streams into the stores. Returns nothing; React
 * StrictMode mounts effects twice in development, so every subscription is
 * disposed on cleanup.
 */
export function useShellEvents(): void {
  const applyEvent = useDownloadsStore((state) => state.applyEvent);
  const upsertMedia = useDownloadsStore((state) => state.upsertMedia);
  const setClipboardUrl = useDownloadsStore((state) => state.setClipboardUrl);
  const setMedia = useDownloadsStore((state) => state.refreshMedia);
  const openDialog = useUiStore((state) => state.openDialog);

  useEffect(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];

    const register = <T,>(promise: Promise<T | null>, dispose: (value: T) => void) => {
      void promise
        .then((value) => {
          if (!value) {
            return;
          }
          if (disposed) {
            dispose(value);
          } else {
            cleanups.push(() => dispose(value));
          }
        })
        .catch((error: unknown) => {
          console.warn("Rilldown event subscription failed", error);
        });
    };

    register(onEngineEvent(applyEvent), (unlisten) => unlisten());
    register(onMediaJob(upsertMedia), (unlisten) => unlisten());
    register(onClipboardUrl(setClipboardUrl), (unlisten) => unlisten());
    register(onOpenSettings(() => openDialog("settings")), (unlisten) => unlisten());
    void setMedia();

    return () => {
      disposed = true;
      for (const cleanup of cleanups.splice(0)) {
        cleanup();
      }
    };
  }, [applyEvent, upsertMedia, setClipboardUrl, openDialog, setMedia]);
}

/** Load persisted records once at startup. */
export function useInitialLoad(): void {
  const refreshAll = useDownloadsStore((state) => state.refreshAll);
  useEffect(() => {
    void refreshAll();
  }, [refreshAll]);
}

/** App-wide keyboard shortcuts. */
export function useShortcuts(): void {
  const openDialog = useUiStore((state) => state.openDialog);
  const openDownloadsFolder = useDownloadsStore((state) => state.run);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const modifier = event.metaKey || event.ctrlKey;
      if (!modifier) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key === "n") {
        event.preventDefault();
        openDialog("add");
      } else if (key === "m") {
        event.preventDefault();
        openDialog("media");
      } else if (key === ",") {
        event.preventDefault();
        openDialog("settings");
      } else if (key === "o") {
        event.preventDefault();
        void openDownloadsFolder("open-folder", async () => {
          await api.openDownloadsFolder();
        });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openDialog, openDownloadsFolder]);
}
