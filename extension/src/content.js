/*
 * Media detection content script.
 *
 * It deliberately does not inject visible UI. It reports what the page already
 * loaded, so the desktop app can decide what is worth downloading.
 */

(() => {
  const MEDIA_EXTENSIONS = /\.(m3u8|mpd|mp4|webm|mkv|mov|ts|mp3|m4a|aac|ogg|flac|wav)(\?|#|$)/i;
  const MAX_CANDIDATES = 50;

  function formatFor(url) {
    const path = url.pathname.toLowerCase();
    if (path.endsWith(".m3u8") || path.endsWith(".m3u")) return "hls";
    if (path.endsWith(".mpd")) return "dash";
    if (/\.(mp4|webm|mkv|mov|ts)$/.test(path)) return "video";
    return "audio";
  }

  function collect() {
    const found = new Map();

    const add = (raw, source) => {
      if (!raw) return;
      try {
        const url = new URL(raw, location.href);
        if (url.protocol !== "http:" && url.protocol !== "https:") return;
        if (!MEDIA_EXTENSIONS.test(url.pathname)) return;
        if (!found.has(url.href)) {
          found.set(url.href, {
            url: url.href,
            format: formatFor(url),
            source,
            title: document.title || undefined,
          });
        }
      } catch {
        // Ignore malformed URLs from page content.
      }
    };

    for (const element of document.querySelectorAll("video, audio, source")) {
      add(element.currentSrc || element.src, element.tagName.toLowerCase());
      for (const source of element.querySelectorAll("source")) {
        add(source.src, "source");
      }
    }

    if (typeof performance.getEntriesByType === "function") {
      for (const entry of performance.getEntriesByType("resource")) {
        if (
          entry.initiatorType === "video" ||
          entry.initiatorType === "audio" ||
          entry.initiatorType === "xmlhttprequest" ||
          entry.initiatorType === "fetch"
        ) {
          add(entry.name, "network");
        }
      }
    }

    return Array.from(found.values()).slice(0, MAX_CANDIDATES);
  }

  function report() {
    const media = collect();
    if (media.length === 0) return;
    try {
      chrome.runtime.sendMessage({ type: "rilldown:media", media });
    } catch {
      // The extension was reloaded; the next scan will reconnect.
    }
  }

  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (!message || message.type !== "rilldown:scan") return false;
    const media = collect();
    sendResponse({ ok: true, media });
    return false;
  });

  report();
  window.addEventListener("load", report, { once: true });
})();
