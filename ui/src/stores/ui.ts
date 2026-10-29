import { create } from "zustand";
import { persist } from "zustand/middleware";

export type LibraryFilter = "all" | "downloading" | "queued" | "completed" | "failed";
export type CategoryId = "compressed" | "documents" | "music" | "video" | "programs" | "other";
export type ThemeMode = "system" | "light" | "dark";
export type Language = "en" | "bn";
export type DialogKind = "add" | "settings" | "media" | null;

export type Selection =
  | { kind: "library"; filter: LibraryFilter }
  | { kind: "category"; id: CategoryId }
  | { kind: "queue"; id: string };

interface UiState {
  selection: Selection;
  search: string;
  theme: ThemeMode;
  language: Language;
  dialog: DialogKind;
  pendingUrl: string | null;
  select: (selection: Selection) => void;
  setSearch: (search: string) => void;
  setTheme: (theme: ThemeMode) => void;
  setLanguage: (language: Language) => void;
  openDialog: (dialog: Exclude<DialogKind, null>) => void;
  openDialogWithUrl: (url: string) => void;
  setPendingUrl: (url: string | null) => void;
  closeDialog: () => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      selection: { kind: "library", filter: "all" },
      search: "",
      theme: "system",
      language: "en",
      dialog: null,
      pendingUrl: null,
      select: (selection) => set({ selection }),
      setSearch: (search) => set({ search }),
      setTheme: (theme) => set({ theme }),
      setLanguage: (language) => set({ language }),
      openDialog: (dialog) => set({ dialog }),
      openDialogWithUrl: (url) => set({ dialog: "add", pendingUrl: url }),
      setPendingUrl: (pendingUrl) => set({ pendingUrl }),
      closeDialog: () => set({ dialog: null }),
    }),
    {
      name: "rilldown.ui",
      partialize: (state) => ({ theme: state.theme, language: state.language }),
    },
  ),
);
