import { useEffect } from "react";

import { DownloadList } from "@/components/downloads/DownloadList";
import { AddDownloadDialog } from "@/components/dialogs/AddDownloadDialog";
import { MediaDialog } from "@/components/dialogs/MediaDialog";
import { SettingsDialog } from "@/components/dialogs/SettingsDialog";
import { Sidebar } from "@/components/layout/Sidebar";
import { StatusBar } from "@/components/layout/StatusBar";
import { Toolbar } from "@/components/layout/Toolbar";
import { ThemeSync } from "@/components/theme-sync";
import { ToastHost } from "@/components/toast-host";
import {
  useInitialLoad,
  useShortcuts,
  useShellEvents,
} from "@/hooks/use-shell-events";
import i18n from "@/i18n";
import { useUiStore } from "@/stores/ui";

export default function App() {
  const language = useUiStore((state) => state.language);

  useInitialLoad();
  useShellEvents();
  useShortcuts();

  useEffect(() => {
    void i18n.changeLanguage(language);
    document.documentElement.lang = language;
  }, [language]);

  return (
    <div className="flex h-full min-h-0 bg-canvas text-ink">
      <ThemeSync />
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <Toolbar />
        <main className="min-h-0 flex-1 overflow-y-auto">
          <DownloadList />
        </main>
        <StatusBar />
      </div>
      <AddDownloadDialog />
      <MediaDialog />
      <SettingsDialog />
      <ToastHost />
    </div>
  );
}
