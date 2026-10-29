/*
 * Rilldown browser bridge.
 *
 * This file is loaded as a classic service worker (Chromium) and as a classic
 * background script (Firefox), so it deliberately contains no imports.
 */

const NATIVE_HOST = "app.rilldown.desktop";
const REQUEST_TIMEOUT_MS = 2500;
const api = globalThis.browser ?? globalThis.chrome;

let nativePort = null;
let sequence = 0;
const pending = new Map();
const mediaByTab = new Map();

function connect() {
  if (nativePort) {
    return nativePort;
  }
  const port = api.runtime.connectNative(NATIVE_HOST);
  nativePort = port;

  port.onMessage.addListener((message) => {
    const id = message && message.id;
    if (id == null) {
      return;
    }
    const entry = pending.get(id);
    if (!entry) {
      return;
    }
    pending.delete(id);
    clearTimeout(entry.timer);
    entry.resolve(message);
  });

  port.onDisconnect.addListener(() => {
    const detail =
      (api.runtime.lastError && api.runtime.lastError.message) ||
      (port.error && port.error.message) ||
      "Rilldown closed the native connection";
    nativePort = null;
    for (const entry of pending.values()) {
      clearTimeout(entry.timer);
      entry.reject(new Error(detail));
    }
    pending.clear();
  });

  return port;
}

function request(method, params = {}) {
  return new Promise((resolve, reject) => {
    let port;
    try {
      port = connect();
    } catch (error) {
      reject(error);
      return;
    }

    const id = `${Date.now()}-${++sequence}`;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error("Rilldown did not answer in time"));
    }, REQUEST_TIMEOUT_MS);
    pending.set(id, { resolve, reject, timer });

    try {
      port.postMessage({ id, method, params });
    } catch (error) {
      clearTimeout(timer);
      pending.delete(id);
      reject(error);
    }
  });
}

async function connectionStatus() {
  try {
    // "status" asks the native host to check the app's authenticated bridge;
    // "ping" only proves that the host process started.
    const response = await request("status");
    const result = (response && response.result) || {};
    return {
      connected: Boolean(response && response.ok && result.connected),
      appRunning: Boolean(result.appRunning),
      version: result.version || null,
      detail: result.detail || null,
    };
  } catch (error) {
    return { connected: false, appRunning: false, error: error.message };
  }
}

async function sendUrl(url, tab) {
  try {
    const response = await request("download", {
      url,
      referrer: tab && tab.url ? tab.url : undefined,
    });
    if (response && response.ok) {
      return { ok: true, result: response.result };
    }
    return {
      ok: false,
      error:
        (response && response.error && response.error.message) ||
        "Rilldown declined the request",
    };
  } catch (error) {
    return { ok: false, error: error.message };
  }
}

function createMenus() {
  const options = { title: "Download with Rilldown" };
  api.contextMenus.create(
    { ...options, id: "rilldown-link", contexts: ["link", "video", "audio"] },
    () => void api.runtime.lastError,
  );
  api.contextMenus.create(
    {
      id: "rilldown-page",
      title: "Send this page to Rilldown",
      contexts: ["page"],
    },
    () => void api.runtime.lastError,
  );
}

api.runtime.onInstalled.addListener(createMenus);
api.runtime.onStartup.addListener(createMenus);

api.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl || info.pageUrl || (tab && tab.url);
  if (url) {
    void sendUrl(url, tab);
  }
});

api.commands.onCommand.addListener(async (command) => {
  if (command !== "send-to-rilldown") {
    return;
  }
  const tabs = await api.tabs.query({ active: true, currentWindow: true });
  const tab = tabs[0];
  if (tab && tab.url) {
    await sendUrl(tab.url, tab);
  }
});

api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || typeof message.type !== "string") {
    return false;
  }

  if (message.type === "rilldown:status") {
    connectionStatus().then(sendResponse);
    return true;
  }

  if (message.type === "rilldown:send") {
    sendUrl(message.url, { url: message.referrer, title: message.title }).then(sendResponse);
    return true;
  }

  if (message.type === "rilldown:media") {
    if (sender.tab && sender.tab.id != null) {
      mediaByTab.set(sender.tab.id, Array.isArray(message.media) ? message.media : []);
    }
    return false;
  }

  if (message.type === "rilldown:scan-tab") {
    const tabId = message.tabId;
    if (tabId == null) {
      sendResponse({ ok: false, error: "No active tab" });
      return true;
    }
    api.tabs
      .sendMessage(tabId, { type: "rilldown:scan" })
      .then((response) => {
        const detected = (response && response.media) || mediaByTab.get(tabId) || [];
        mediaByTab.set(tabId, detected);
        sendResponse({ ok: true, media: detected });
      })
      .catch((error) => {
        sendResponse({
          ok: false,
          media: mediaByTab.get(tabId) || [],
          error: error.message,
        });
      });
    return true;
  }

  return false;
});
