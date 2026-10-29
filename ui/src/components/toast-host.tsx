import { AlertCircle, Clipboard, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { useDownloadsStore } from "@/stores/downloads";
import { useUiStore } from "@/stores/ui";

export function ToastHost() {
  const { t } = useTranslation();
  const clipboardUrl = useDownloadsStore((state) => state.clipboardUrl);
  const setClipboardUrl = useDownloadsStore((state) => state.setClipboardUrl);
  const error = useDownloadsStore((state) => state.error);
  const clearError = useDownloadsStore((state) => state.clearError);

  if (!clipboardUrl && !error) {
    return null;
  }

  return (
    <div className="pointer-events-none fixed bottom-11 right-4 z-40 flex w-[min(380px,calc(100vw-2rem))] flex-col gap-2">
      {clipboardUrl ? (
        <div
          role="status"
          className="pointer-events-auto flex items-start gap-2.5 rounded-xl border border-line bg-surface p-3 shadow-soft"
        >
          <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
            <Clipboard className="h-3.5 w-3.5" aria-hidden="true" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="text-[12px] font-medium text-ink">
              {t("clipboard.title", { defaultValue: "Link copied" })}
            </p>
            <p className="mt-0.5 truncate text-[11px] text-muted">{clipboardUrl}</p>
            <div className="mt-2 flex items-center gap-2">
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  useUiStore.getState().openDialogWithUrl(clipboardUrl);
                  setClipboardUrl(null);
                }}
              >
                {t("clipboard.add", { defaultValue: "Download it" })}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setClipboardUrl(null)}>
                {t("clipboard.dismiss", { defaultValue: "Dismiss" })}
              </Button>
            </div>
          </div>
          <IconButton
            icon={X}
            label={t("common.close", { defaultValue: "Close" })}
            onClick={() => setClipboardUrl(null)}
          />
        </div>
      ) : null}

      {error ? (
        <div
          role="alert"
          className="pointer-events-auto flex items-start gap-2.5 rounded-xl border border-danger/30 bg-surface p-3 shadow-soft"
        >
          <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-danger/10 text-danger">
            <AlertCircle className="h-3.5 w-3.5" aria-hidden="true" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="text-[12px] font-medium text-ink">
              {t("errors.title", { defaultValue: "Something went wrong" })}
            </p>
            <p className="mt-0.5 line-clamp-3 break-words text-[11px] leading-5 text-muted">
              {error}
            </p>
          </div>
          <IconButton
            icon={X}
            label={t("common.dismiss", { defaultValue: "Dismiss" })}
            onClick={clearError}
          />
        </div>
      ) : null}
    </div>
  );
}
