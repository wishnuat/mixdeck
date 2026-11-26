// Isolated-world content script: forwards the tab's gain from the extension to page-gain.js (MAIN world).
let gain = null;
const push = () => gain !== null && document.dispatchEvent(new CustomEvent("mixdeck:gain", { detail: gain }));

chrome.runtime.onMessage.addListener((m) => {
  if (m && m.type === "mixdeck:gain") {
    gain = m.gain;
    push();
  }
});

// page-gain.js may initialise before or after us: answer its "ready" too.
document.addEventListener("mixdeck:ready", push);

chrome.runtime.sendMessage({ type: "mixdeck:hello" }, (r) => {
  void chrome.runtime.lastError; // extension reloading: ignore
  if (r && typeof r.gain === "number") {
    gain = r.gain;
    push();
  }
});
