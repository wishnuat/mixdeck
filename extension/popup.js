const list = document.getElementById("list");
let dragging = false;

function render({ tabs, connected }) {
  document.getElementById("dot").classList.toggle("on", connected);
  document.getElementById("st").textContent = connected ? "Linked to Mixdeck" : "Mixdeck app not running";
  if (dragging) return;
  list.innerHTML = "";
  if (!tabs.length) {
    list.innerHTML = '<div class="empty">No tab is playing sound right now. Tabs show up here (and in the Mixdeck tray mixer) as soon as they make noise.</div>';
    return;
  }
  for (const t of tabs) {
    const li = document.createElement("li");
    const fav = t.favIconUrl ? Object.assign(document.createElement("img"), { src: t.favIconUrl }) : Object.assign(document.createElement("span"), { className: "fav" });
    const text = document.createElement("div");
    text.innerHTML = '<div class="title"></div><div class="host"></div>';
    text.querySelector(".title").textContent = t.title;
    text.querySelector(".host").textContent = t.host;
    const mute = Object.assign(document.createElement("button"), { className: "m", title: t.muted ? "Unmute tab" : "Mute tab", textContent: t.muted ? "🔇" : "🔊" });
    mute.onclick = () => chrome.runtime.sendMessage({ type: "setMute", tabId: t.id, muted: !t.muted }).then(refresh);
    const range = Object.assign(document.createElement("input"), { type: "range", min: 0, max: 100, value: Math.round(t.volume * 100) });
    const pct = Object.assign(document.createElement("div"), { className: "pct", textContent: Math.round(t.volume * 100) });
    range.oninput = () => {
      dragging = true;
      pct.textContent = range.value;
      chrome.runtime.sendMessage({ type: "setVolume", tabId: t.id, volume: range.value / 100 });
    };
    range.onchange = () => { dragging = false; };
    li.append(fav, text, mute, range, pct);
    list.append(li);
  }
}

function refresh() {
  chrome.runtime.sendMessage({ type: "mixdeck:list" }, (r) => r && render(r));
}
chrome.runtime.onMessage.addListener((m) => { if (m?.type === "mixdeck:popup") render(m); });
refresh();
