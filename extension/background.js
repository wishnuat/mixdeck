// Mixdeck extension service worker.
//  - keeps the list of tabs that are (or were recently) playing sound
//  - stores per-tab gain, remembered per site (youtube.com → 40 %)
//  - talks to the Mixdeck app through native messaging (host "id.wishnu.mixdeck")

const HOST = "id.wishnu.mixdeck";
const RECENT_MS = 30_000; // keep a tab listed for a while after it goes quiet

const tabGain = new Map(); // tabId → gain
const lastAudible = new Map(); // tabId → timestamp
let siteGain = {}; // host → gain (persisted)
let port = null;
let retryMs = 2000;
let publishTimer = null;
let connected = false;

const hostOf = (url) => { try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return ""; } };
const gainFor = (tab) => tabGain.get(tab.id) ?? siteGain[hostOf(tab.url || "")] ?? 1;

chrome.storage.local.get("siteGain").then((r) => { siteGain = r.siteGain || {}; });

// ---------------------------------------------------------------- tab list

async function listTabs() {
  const now = Date.now();
  const tabs = await chrome.tabs.query({});
  return tabs
    .filter((t) => {
      if (t.audible) lastAudible.set(t.id, now);
      const recent = now - (lastAudible.get(t.id) ?? 0) < RECENT_MS;
      return t.audible || recent || t.mutedInfo?.muted || tabGain.has(t.id);
    })
    .map((t) => ({
      id: t.id,
      title: t.title || hostOf(t.url || "") || "Tab",
      host: hostOf(t.url || ""),
      favIconUrl: t.favIconUrl || "",
      audible: !!t.audible,
      muted: !!t.mutedInfo?.muted,
      volume: gainFor(t),
      active: !!t.active,
    }));
}

function publishSoon(delay = 120) {
  clearTimeout(publishTimer);
  publishTimer = setTimeout(async () => {
    const tabs = await listTabs();
    send({ type: "tabs", tabs });
    chrome.runtime.sendMessage({ type: "mixdeck:popup", tabs, connected }).catch(() => {});
  }, delay);
}

chrome.tabs.onUpdated.addListener((id, info) => {
  if ("audible" in info || "mutedInfo" in info || "title" in info || "favIconUrl" in info || "url" in info) publishSoon();
});
chrome.tabs.onRemoved.addListener((id) => { tabGain.delete(id); lastAudible.delete(id); publishSoon(); });
setInterval(() => publishSoon(0), 5000); // drops tabs whose "recently audible" window has passed

// ---------------------------------------------------------------- actions

async function setVolume(tabId, volume) {
  const g = Math.max(0, Math.min(1, Number(volume)));
  if (Number.isNaN(g)) return;
  tabGain.set(tabId, g);
  try {
    const tab = await chrome.tabs.get(tabId);
    const host = hostOf(tab.url || "");
    if (host) {
      siteGain[host] = g;
      chrome.storage.local.set({ siteGain });
    }
    await chrome.tabs.sendMessage(tabId, { type: "mixdeck:gain", gain: g });
  } catch { /* tab gone or no content script (chrome:// pages) */ }
  publishSoon();
}

async function setMute(tabId, muted) {
  try { await chrome.tabs.update(tabId, { muted: !!muted }); } catch {}
  publishSoon();
}

function handle(msg) {
  if (!msg || typeof msg !== "object") return;
  if (msg.type === "setVolume") setVolume(msg.tabId, msg.volume);
  else if (msg.type === "setMute") setMute(msg.tabId, msg.muted);
  else if (msg.type === "refresh") publishSoon(0);
}

// content scripts ask for their tab's gain when a page loads; the popup uses the same actions
chrome.runtime.onMessage.addListener((m, sender, reply) => {
  if (m?.type === "mixdeck:hello" && sender.tab) {
    const host = hostOf(sender.tab.url || "");
    // a navigation to another site resets to that site's remembered level
    const known = tabGain.get(sender.tab.id);
    const g = siteGain[host] ?? known ?? 1;
    if (siteGain[host] !== undefined) tabGain.set(sender.tab.id, g);
    reply({ gain: g });
    return;
  }
  if (m?.type === "mixdeck:list") {
    listTabs().then((tabs) => reply({ tabs, connected }));
    return true; // async reply
  }
  if (m?.type === "setVolume" || m?.type === "setMute") handle(m);
});

// ---------------------------------------------------------------- native messaging

function send(msg) {
  if (!port) return;
  try { port.postMessage(msg); } catch { /* disconnected */ }
}

function connect() {
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch {
    port = null;
    return setTimeout(connect, retryMs);
  }
  connected = true;
  port.onMessage.addListener((m) => { retryMs = 2000; handle(m); });
  port.onDisconnect.addListener(() => {
    void chrome.runtime.lastError; // e.g. host not installed
    port = null;
    connected = false;
    retryMs = Math.min(retryMs * 2, 60_000);
    setTimeout(connect, retryMs);
  });
  publishSoon(0);
}

connect();
