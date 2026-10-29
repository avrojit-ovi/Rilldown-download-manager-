import { ChevronDown, FolderOpen } from "lucide-react";
import { useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { api, type AddDownloadInput, type BatchOutcome } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useDownloadsStore } from "@/stores/downloads";
import { useUiStore } from "@/stores/ui";

type Mode = "single" | "batch";

const MAX_BATCH = 200;

const selectClass =
  "h-9 w-full rounded-lg border border-line bg-surface px-3 text-[13px] text-ink transition-colors focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/15 disabled:opacity-50";

const textareaClass =
  "w-full rounded-lg border border-line bg-surface px-3 py-2 text-[13px] text-ink placeholder:text-muted/70 transition-colors focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/15 disabled:opacity-50";

function isHttpUrl(value: string): boolean {
  return /^https?:\/\/\S+$/i.test(value.trim());
}

function messageOf(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function parseHeaders(raw: string): { headers?: Record<string, string>; error?: string } {
  const headers: Record<string, string> = {};
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) {
      continue;
    }
    const separator = trimmed.indexOf(":");
    if (separator <= 0) {
      return { error: `Invalid header line: “${trimmed}”. Use Name: value.` };
    }
    const name = trimmed.slice(0, separator).trim();
    const value = trimmed.slice(separator + 1).trim();
    if (!name) {
      return { error: `Invalid header line: “${trimmed}”. The name is empty.` };
    }
    headers[name] = value;
  }
  return Object.keys(headers).length > 0 ? { headers } : {};
}

export function AddDownloadDialog() {
  const { t } = useTranslation();
  const dialog = useUiStore((state) => state.dialog);
  const closeDialog = useUiStore((state) => state.closeDialog);
  const setPendingUrl = useUiStore((state) => state.setPendingUrl);
  const settings = useDownloadsStore((state) => state.settings);
  const queues = useDownloadsStore((state) => state.queues);
  const refresh = useDownloadsStore((state) => state.refresh);
  const refreshQueues = useDownloadsStore((state) => state.refreshQueues);
  const open = dialog === "add";

  const fieldId = useId();
  const [mode, setMode] = useState<Mode>("single");
  const [url, setUrl] = useState("");
  const [filename, setFilename] = useState("");
  const [saveDir, setSaveDir] = useState("");
  const [queueId, setQueueId] = useState("main");
  const [connections, setConnections] = useState("8");
  const [speedLimitKib, setSpeedLimitKib] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const [referrer, setReferrer] = useState("");
  const [cookies, setCookies] = useState("");
  const [headersText, setHeadersText] = useState("");
  const [batchText, setBatchText] = useState("");
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [results, setResults] = useState<BatchOutcome[]>([]);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!open) {
      return;
    }
    const state = useDownloadsStore.getState();
    setMode("single");
    setUrl(useUiStore.getState().pendingUrl ?? "");
    useUiStore.getState().setPendingUrl(null);
    setFilename("");
    setSaveDir(state.settings?.downloadDir ?? "");
    setQueueId(state.queues[0]?.id ?? "main");
    setConnections(String(state.settings?.connections ?? 8));
    setSpeedLimitKib("");
    setAdvanced(false);
    setReferrer("");
    setCookies("");
    setHeadersText("");
    setBatchText("");
    setFieldError(null);
    setFormError(null);
    setResults([]);
    setSubmitting(false);
  }, [open, setPendingUrl]);

  useEffect(() => {
    if (open) {
      void refreshQueues();
    }
  }, [open, refreshQueues]);

  const batchUrls = useMemo(
    () =>
      batchText
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0 && !line.startsWith("#")),
    [batchText],
  );

  const queueOptions: { id: string; name: string }[] =
    queues.length > 0
      ? queues
      : [{ id: "main", name: t("nav.mainQueue", { defaultValue: "Main queue" }) }];

  const browse = async () => {
    try {
      const directory = await api.chooseDirectory();
      if (directory) {
        setSaveDir(directory);
      }
    } catch (error) {
      setFormError(messageOf(error));
    }
  };

  const buildInput = (target: string, includeExtras: boolean): AddDownloadInput => {
    const input: AddDownloadInput = { url: target };
    if (includeExtras) {
      const trimmedName = filename.trim();
      if (trimmedName) {
        input.filename = trimmedName;
      }
    }
    const trimmedDir = saveDir.trim();
    if (trimmedDir) {
      input.saveDir = trimmedDir;
    }
    if (queueId) {
      input.queueId = queueId;
    }
    input.connections = Number(connections);
    if (includeExtras) {
      if (speedLimitKib.trim()) {
        input.speedLimitBps = Math.round(Number(speedLimitKib) * 1024);
      } else {
        input.speedLimitBps = null;
      }
    }
    if (includeExtras && advanced) {
      if (referrer.trim()) {
        input.referrer = referrer.trim();
      }
      if (cookies.trim()) {
        input.cookies = cookies.trim();
      }
      const parsed = parseHeaders(headersText);
      if (parsed.headers) {
        input.headers = parsed.headers;
      }
    }
    return input;
  };

  const validateCommon = (): string | null => {
    if (!Number.isInteger(Number(connections)) || Number(connections) < 1 || Number(connections) > 32) {
      return t("add.connectionsError", {
        defaultValue: "Connections must be a whole number between 1 and 32.",
      });
    }
    if (mode === "single" && speedLimitKib.trim()) {
      const limit = Number(speedLimitKib);
      if (!Number.isFinite(limit) || limit <= 0) {
        return t("add.speedError", {
          defaultValue: "The speed limit must be a positive number of KiB/s, or left empty.",
        });
      }
    }
    if (mode === "single" && advanced) {
      const parsed = parseHeaders(headersText);
      if (parsed.error) {
        return parsed.error;
      }
    }
    return null;
  };

  const submitSingle = async () => {
    const target = url.trim();
    if (!target) {
      setFieldError(t("add.urlRequired", { defaultValue: "Enter a URL to download." }));
      return;
    }
    if (!isHttpUrl(target)) {
      setFieldError(
        t("add.urlInvalid", { defaultValue: "Only http:// and https:// URLs are supported." }),
      );
      return;
    }
    setFieldError(null);
    const commonError = validateCommon();
    if (commonError) {
      setFormError(commonError);
      return;
    }
    setFormError(null);
    setSubmitting(true);
    try {
      await api.addDownload(buildInput(target, true));
      await refresh();
      closeDialog();
    } catch (error) {
      setFormError(messageOf(error));
    } finally {
      setSubmitting(false);
    }
  };

  const submitBatch = async () => {
    if (batchUrls.length === 0) {
      setFieldError(
        t("add.batchEmpty", { defaultValue: "Paste at least one http:// or https:// URL." }),
      );
      return;
    }
    if (batchUrls.length > MAX_BATCH) {
      setFieldError(
        t("add.batchTooMany", {
          count: MAX_BATCH,
          defaultValue: "Add at most {{count}} URLs at a time.",
        }),
      );
      return;
    }
    setFieldError(null);
    const commonError = validateCommon();
    if (commonError) {
      setFormError(commonError);
      return;
    }
    setFormError(null);
    setResults([]);
    setSubmitting(true);

    const invalid: BatchOutcome[] = batchUrls
      .filter((candidate) => !isHttpUrl(candidate))
      .map((candidate) => ({
        url: candidate,
        ok: false,
        record: null,
        error: t("add.urlInvalid", {
          defaultValue: "Only http:// and https:// URLs are supported.",
        }),
      }));
    const valid = batchUrls.filter(isHttpUrl);

    try {
      const outcomes =
        valid.length > 0
          ? await api.addDownloads(valid.map((candidate) => buildInput(candidate, false)))
          : [];
      const combined = [...invalid, ...outcomes];
      setResults(combined);
      await refresh();
      const failures = combined.filter((outcome) => !outcome.ok);
      if (failures.length === 0) {
        closeDialog();
      }
    } catch (error) {
      setFormError(messageOf(error));
    } finally {
      setSubmitting(false);
    }
  };

  const submit = () => {
    if (submitting) {
      return;
    }
    void (mode === "single" ? submitSingle() : submitBatch());
  };

  const failures = results.filter((outcome) => !outcome.ok);
  const successes = results.filter((outcome) => outcome.ok);
  const canSubmit =
    !submitting && (mode === "single" ? url.trim().length > 0 : batchUrls.length > 0);

  return (
    <Dialog
      open={open}
      title={t("add.title", { defaultValue: "Add download" })}
      description={t("add.description", {
        defaultValue:
          "Paste a direct link, or queue a batch. Rilldown probes the file before it starts.",
      })}
      onClose={() => {
        if (!submitting) {
          closeDialog();
        }
      }}
      className="max-w-[620px]"
      footer={
        <>
          <Button variant="ghost" onClick={closeDialog} disabled={submitting}>
            {t("actions.cancel", { defaultValue: "Cancel" })}
          </Button>
          <Button variant="primary" onClick={submit} disabled={!canSubmit}>
            {submitting
              ? t("add.submitting", { defaultValue: "Adding…" })
              : mode === "single"
                ? t("add.addOne", { defaultValue: "Add download" })
                : t("add.addCount", {
                    count: batchUrls.length,
                    defaultValue: "Add {{count}} downloads",
                  })}
          </Button>
        </>
      }
    >
      <div className="space-y-4">
        <div
          role="tablist"
          aria-label={t("add.mode", { defaultValue: "Add mode" })}
          className="inline-flex rounded-lg border border-line bg-surface-2 p-0.5"
        >
          {(["single", "batch"] as const).map((candidate) => (
            <button
              key={candidate}
              type="button"
              role="tab"
              aria-selected={mode === candidate}
              onClick={() => {
                setMode(candidate);
                setFieldError(null);
                setFormError(null);
                setResults([]);
              }}
              className={cn(
                "h-7 rounded-[7px] px-3 text-[12px] font-medium transition-colors",
                mode === candidate
                  ? "bg-surface text-ink shadow-sm"
                  : "text-muted hover:text-ink",
              )}
            >
              {candidate === "single"
                ? t("add.tabSingle", { defaultValue: "One URL" })
                : t("add.tabBatch", { defaultValue: "Batch" })}
            </button>
          ))}
        </div>

        {mode === "single" ? (
          <div className="space-y-3">
            <div>
              <label
                htmlFor={`${fieldId}-url`}
                className="mb-1.5 block text-[12px] font-medium text-ink"
              >
                {t("add.url", { defaultValue: "Download URL" })}
              </label>
              <Input
                id={`${fieldId}-url`}
                type="url"
                inputMode="url"
                autoComplete="off"
                spellCheck={false}
                placeholder="https://example.com/file.zip"
                value={url}
                onChange={(event) => {
                  setUrl(event.target.value);
                  if (fieldError) {
                    setFieldError(null);
                  }
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    submit();
                  }
                }}
                aria-invalid={fieldError ? true : undefined}
                aria-describedby={fieldError ? `${fieldId}-error` : undefined}
              />
              {fieldError ? (
                <p id={`${fieldId}-error`} className="mt-1.5 text-[11px] text-danger">
                  {fieldError}
                </p>
              ) : null}
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div>
                <label
                  htmlFor={`${fieldId}-filename`}
                  className="mb-1.5 block text-[12px] font-medium text-ink"
                >
                  {t("add.fileName", { defaultValue: "File name" })}{" "}
                  <span className="font-normal text-muted">
                    {t("add.optional", { defaultValue: "(optional)" })}
                  </span>
                </label>
                <Input
                  id={`${fieldId}-filename`}
                  placeholder={t("add.fileNamePlaceholder", { defaultValue: "Keep server name" })}
                  value={filename}
                  onChange={(event) => setFilename(event.target.value)}
                />
              </div>
              <div>
                <label
                  htmlFor={`${fieldId}-queue`}
                  className="mb-1.5 block text-[12px] font-medium text-ink"
                >
                  {t("add.queue", { defaultValue: "Queue" })}
                </label>
                <select
                  id={`${fieldId}-queue`}
                  className={selectClass}
                  value={queueId}
                  onChange={(event) => setQueueId(event.target.value)}
                >
                  {queueOptions.map((queue) => (
                    <option key={queue.id} value={queue.id}>
                      {queue.name}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div>
              <label
                htmlFor={`${fieldId}-save`}
                className="mb-1.5 block text-[12px] font-medium text-ink"
              >
                {t("add.saveTo", { defaultValue: "Save to" })}
              </label>
              <div className="flex gap-2">
                <Input
                  id={`${fieldId}-save`}
                  value={saveDir}
                  onChange={(event) => setSaveDir(event.target.value)}
                  placeholder={settings?.downloadDir ?? ""}
                />
                <Button variant="secondary" onClick={() => void browse()}>
                  <FolderOpen className="h-4 w-4" aria-hidden="true" />
                  {t("actions.browse", { defaultValue: "Browse" })}
                </Button>
              </div>
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div>
                <label
                  htmlFor={`${fieldId}-connections`}
                  className="mb-1.5 block text-[12px] font-medium text-ink"
                >
                  {t("add.connections", { defaultValue: "Connections" })}
                </label>
                <Input
                  id={`${fieldId}-connections`}
                  type="number"
                  min={1}
                  max={32}
                  value={connections}
                  onChange={(event) => setConnections(event.target.value)}
                />
              </div>
              <div>
                <label
                  htmlFor={`${fieldId}-speed`}
                  className="mb-1.5 block text-[12px] font-medium text-ink"
                >
                  {t("add.speedLimit", { defaultValue: "Speed limit (KiB/s)" })}
                </label>
                <Input
                  id={`${fieldId}-speed`}
                  type="number"
                  min={1}
                  placeholder={t("add.unlimited", { defaultValue: "Unlimited" })}
                  value={speedLimitKib}
                  onChange={(event) => setSpeedLimitKib(event.target.value)}
                />
              </div>
            </div>

            <div className="rounded-lg border border-line">
              <button
                type="button"
                aria-expanded={advanced}
                onClick={() => setAdvanced((value) => !value)}
                className="flex h-9 w-full items-center justify-between px-3 text-[12px] font-medium text-muted transition-colors hover:text-ink"
              >
                {t("add.advanced", { defaultValue: "Advanced: referrer, cookies, headers" })}
                <ChevronDown
                  className={cn("h-4 w-4 transition-transform", advanced && "rotate-180")}
                  aria-hidden="true"
                />
              </button>
              {advanced ? (
                <div className="space-y-3 border-t border-line px-3 pb-3 pt-3">
                  <div>
                    <label
                      htmlFor={`${fieldId}-referrer`}
                      className="mb-1.5 block text-[12px] font-medium text-ink"
                    >
                      {t("add.referrer", { defaultValue: "Referrer" })}
                    </label>
                    <Input
                      id={`${fieldId}-referrer`}
                      placeholder="https://example.com/page"
                      value={referrer}
                      onChange={(event) => setReferrer(event.target.value)}
                    />
                  </div>
                  <div>
                    <label
                      htmlFor={`${fieldId}-cookies`}
                      className="mb-1.5 block text-[12px] font-medium text-ink"
                    >
                      {t("add.cookies", { defaultValue: "Cookie header" })}
                    </label>
                    <Input
                      id={`${fieldId}-cookies`}
                      placeholder="session=abc; theme=dark"
                      value={cookies}
                      onChange={(event) => setCookies(event.target.value)}
                    />
                  </div>
                  <div>
                    <label
                      htmlFor={`${fieldId}-headers`}
                      className="mb-1.5 block text-[12px] font-medium text-ink"
                    >
                      {t("add.headers", { defaultValue: "Custom headers" })}
                    </label>
                    <textarea
                      id={`${fieldId}-headers`}
                      rows={3}
                      spellCheck={false}
                      className={textareaClass}
                      placeholder={"Authorization: Bearer …\nAccept-Language: en"}
                      value={headersText}
                      onChange={(event) => setHeadersText(event.target.value)}
                    />
                    <p className="mt-1 text-[11px] text-muted">
                      {t("add.headersHint", {
                        defaultValue: "One Name: value pair per line. Sent only with this download.",
                      })}
                    </p>
                  </div>
                </div>
              ) : null}
            </div>
          </div>
        ) : (
          <div className="space-y-3">
            <div>
              <div className="mb-1.5 flex items-center justify-between gap-2">
                <label
                  htmlFor={`${fieldId}-batch`}
                  className="block text-[12px] font-medium text-ink"
                >
                  {t("add.batchLabel", { defaultValue: "One URL per line" })}
                </label>
                <span className="text-[11px] tabular-nums text-muted">
                  {t("add.batchCount", {
                    count: batchUrls.length,
                    defaultValue: "{{count}} URLs",
                  })}
                </span>
              </div>
              <textarea
                id={`${fieldId}-batch`}
                rows={8}
                spellCheck={false}
                className={cn(textareaClass, "font-mono text-[12px]")}
                placeholder={"https://example.com/a.zip\nhttps://example.com/b.iso"}
                value={batchText}
                onChange={(event) => {
                  setBatchText(event.target.value);
                  if (fieldError) {
                    setFieldError(null);
                  }
                }}
              />
              <p className="mt-1 text-[11px] leading-5 text-muted">
                {t("add.batchHint", {
                  count: MAX_BATCH,
                  defaultValue:
                    "Blank lines and lines starting with # are ignored. Up to {{count}} links, all using the settings below.",
                })}
              </p>
              {fieldError ? <p className="mt-1.5 text-[11px] text-danger">{fieldError}</p> : null}
            </div>

            <div className="grid gap-3 sm:grid-cols-2">
              <div>
                <label
                  htmlFor={`${fieldId}-batch-save`}
                  className="mb-1.5 block text-[12px] font-medium text-ink"
                >
                  {t("add.saveTo", { defaultValue: "Save to" })}
                </label>
                <div className="flex gap-2">
                  <Input
                    id={`${fieldId}-batch-save`}
                    value={saveDir}
                    onChange={(event) => setSaveDir(event.target.value)}
                    placeholder={settings?.downloadDir ?? ""}
                  />
                  <Button variant="secondary" onClick={() => void browse()}>
                    <FolderOpen className="h-4 w-4" aria-hidden="true" />
                  </Button>
                </div>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label
                    htmlFor={`${fieldId}-batch-connections`}
                    className="mb-1.5 block text-[12px] font-medium text-ink"
                  >
                    {t("add.connections", { defaultValue: "Connections" })}
                  </label>
                  <Input
                    id={`${fieldId}-batch-connections`}
                    type="number"
                    min={1}
                    max={32}
                    value={connections}
                    onChange={(event) => setConnections(event.target.value)}
                  />
                </div>
                <div>
                  <label
                    htmlFor={`${fieldId}-batch-queue`}
                    className="mb-1.5 block text-[12px] font-medium text-ink"
                  >
                    {t("add.queue", { defaultValue: "Queue" })}
                  </label>
                  <select
                    id={`${fieldId}-batch-queue`}
                    className={selectClass}
                    value={queueId}
                    onChange={(event) => setQueueId(event.target.value)}
                  >
                    {queueOptions.map((queue) => (
                      <option key={queue.id} value={queue.id}>
                        {queue.name}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
            </div>
          </div>
        )}

        {formError ? (
          <div
            role="alert"
            className="rounded-lg border border-danger/30 bg-danger/5 px-3 py-2 text-[12px] leading-5 text-danger"
          >
            {formError}
          </div>
        ) : null}

        {mode === "batch" && results.length > 0 ? (
          <div className="space-y-2 rounded-lg border border-line bg-surface-2/50 px-3 py-2.5">
            <p className="text-[12px] font-medium text-ink">
              {t("add.batchResult", {
                ok: successes.length,
                total: results.length,
                defaultValue: "{{ok}} of {{total}} downloads queued.",
              })}
            </p>
            {failures.length > 0 ? (
              <ul className="max-h-32 space-y-1 overflow-y-auto">
                {failures.map((failure, index) => (
                  <li key={`${failure.url}-${index}`} className="text-[11px] leading-5 text-danger">
                    <span className="break-all">{failure.url}</span>
                    {failure.error ? <span className="text-muted"> — {failure.error}</span> : null}
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}
      </div>
    </Dialog>
  );
}
