/* Rilldown popup controller. Classic script for both Chromium and Firefox. */

const api = globalThis.browser ?? globalThis.chrome;
const statusDot = document.getElementById("status-dot");
const statusTitle = document.getElementById("status-title");
const statusDetail = document.getElementById("status-detail");
const pageTitle = document.getElementById("page-title");
const pageHost = document.getElementById("page-host");
const mediaCount = document.getElementById("media-count");
const mediaList = document.getElementById("media-list");
const message = document.getElementById("message");

let activeTab = null;

function setMessage(text, tone = "muted") {
  message.textContent = text;
  message.style.color = tone === "error" ? "var(--warning)" : "var(--muted)";
}

async function currentTab() {
  const tabs = await api.tabs.query({ active: true, currentWindow: true });
  return tabs[0] || null;
}

function renderMedia(items) {
  mediaCount.textContent = String(items.length);
  mediaList.replaceChildren();
  if (items.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = "No media found on this page.";
    mediaList.append(empty);
    return;
  }

  for (const item of items.slice(0, 8)) {
    const row = document.createElement("li");
    const kind = document.createElement("span");
    kind.className = "kind";
    kind.textContent = item.format || "media";
    const label = document.createElement("span");
    label.className = "media-label";
    label.textContent = item.name || item.url;
    const send = document.createElement("button");
    send.type = "button";
    send.className = "media-send";
    send.textContent = "Send";
    send.addEventListener("click", async () => {
      setMessage("Sending…");
      const result = await api.runtime.sendMessage({
        type: "rilldown:send",
        url: item.url,
        referrer: item.referrer || (activeTab && activeTab.url),
        title: item.title,
      });
      if (result && result.ok) {
        setMessage("Sent to Rilldown.");
      } else {
        setMessage((result && result.error) || "Rilldown declined the request.", "error");
      }
    });
    row.append(kind, label, send);
    row.title = item.url;
    mediaList.append(row);
  }
}

async function refresh() {
  activeTab = await currentTab();
  if (activeTab) {
    pageTitle.textContent = activeTab.title || "Untitled page";
    try {
      pageHost.textContent = new URL(activeTab.url).hostname;
    } catch {
      pageHost.textContent = "";
    }
  }

  const status = await api.runtime.sendMessage({ type: "rilldown:status" });
  const connected = Boolean(status && status.connected);
  statusDot.classList.toggle("connected", connected);
  statusTitle.textContent = connected ? "Desktop app connected" : "Desktop app not running";
  statusDetail.textContent = connected
    ? `Rilldown ${status.version || ""} is ready to receive downloads.`
    : (status && (status.detail || status.error)) ||
      "Open Rilldown, then use its Settings → Browser integration panel.";

  if (activeTab && activeTab.id != null) {
    const scan = await api.runtime.sendMessage({
      type: "rilldown:scan-tab",
      tabId: activeTab.id,
    });
    renderMedia((scan && scan.media) || []);
  }
}

document.getElementById("refresh").addEventListener("click", () => {
  setMessage("Refreshing…");
  refresh().catch((error) => setMessage(error.message, "error"));
});

document.getElementById("send-page").addEventListener("click", async () => {
  if (!activeTab || !activeTab.url) {
    setMessage("There is no active page to send.", "error");
    return;
  }
  setMessage("Sending…");
  const result = await api.runtime.sendMessage({
    type: "rilldown:send",
    url: activeTab.url,
    referrer: activeTab.url,
    title: activeTab.title,
  });
  if (result && result.ok) {
    setMessage("Sent to Rilldown.");
  } else {
    setMessage(
      (result && result.error) || "Rilldown desktop integration is not available yet.",
      "error",
    );
  }
});

refresh().catch((error) => setMessage(error.message, "error"));
