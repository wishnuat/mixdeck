"""Full end-to-end: real Chromium + the unpacked extension + native messaging + a running Mixdeck app.

Prerequisite: the Mixdeck app is running (it registers the native host and starts the tab hub), started with
MIXDECK_TRACE=1 and stderr redirected to the log passed as argv[1]. Everything runs headless.

    python extension/tests/test_e2e.py <mixdeck-stderr-log>
"""
import os
import sys
import tempfile
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

EXT = str(Path(__file__).resolve().parent.parent)
LOG = Path(sys.argv[1]) if len(sys.argv) > 1 else None
PAGE = "<html><head><title>Mixdeck E2E radio</title></head><body><audio id=a></audio></body></html>"

failures = []


def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + (f"  ({detail})" if detail else ""), flush=True)
    if not cond:
        failures.append(name)


def wait_for(fn, timeout=15):
    t0 = time.time()
    while time.time() - t0 < timeout:
        v = fn()
        if v:
            return v
        time.sleep(0.25)
    return None


with sync_playwright() as p:
    ctx = p.chromium.launch_persistent_context(
        tempfile.mkdtemp(prefix="mixdeck-e2e-"),
        channel="chromium",  # bundled Chromium: supports --load-extension in new headless mode
        headless=True,
        args=[f"--disable-extensions-except={EXT}", f"--load-extension={EXT}", "--autoplay-policy=no-user-gesture-required"],
    )
    sw = ctx.service_workers[0] if ctx.service_workers else ctx.wait_for_event("serviceworker", timeout=15000)
    check("extension service worker started", "pkjnmnolfphodjljfffplljmkgeppgjk" in sw.url, sw.url)

    page = ctx.new_page()
    page.route("http://mixdeck.test/", lambda r: r.fulfill(body=PAGE, content_type="text/html"))
    page.goto("http://mixdeck.test/")
    page.evaluate("document.getElementById('a').volume = 0.8")
    check("content script injected (MAIN world)", page.evaluate("!!window.__mixdeckGain"))

    print("step: query tab", flush=True)
    tab_id = sw.evaluate("chrome.tabs.query({url: 'http://mixdeck.test/*'}).then(t => t[0].id)")

    # extension → page: per-tab gain through relay.js into page-gain.js
    print(f"step: setVolume on tab {tab_id}", flush=True)
    sw.evaluate(f"void setVolume({tab_id}, 0.5)")
    g = wait_for(lambda: page.evaluate("window.__mixdeckGain.gain") == 0.5 and 0.5)
    check("setVolume reaches the page", g == 0.5, f"gain={page.evaluate('window.__mixdeckGain.gain')}")
    check("site still sees its own volume", page.evaluate("document.getElementById('a').volume") == 0.8)

    # remembered per site: a reload starts at the site's level
    page.reload()
    g = wait_for(lambda: page.evaluate("window.__mixdeckGain && window.__mixdeckGain.gain") == 0.5 and 0.5)
    check("site volume restored after reload", g == 0.5, f"gain={page.evaluate('window.__mixdeckGain.gain')}")

    # mute is native (chrome.tabs) and lists the tab
    sw.evaluate(f"void setMute({tab_id}, true)")
    muted = wait_for(lambda: sw.evaluate(f"chrome.tabs.get({tab_id}).then(t => t.mutedInfo.muted)"))
    check("setMute mutes the tab", muted is True)
    listed = sw.evaluate("listTabs()")
    check("muted tab is listed for the mixer", any(t["id"] == tab_id and t["muted"] for t in listed), str(listed))

    # native messaging → bridge → Mixdeck app
    if LOG:
        connected = wait_for(lambda: sw.evaluate("connected"), timeout=20)
        check("extension connected to the native host", connected is True)
        sw.evaluate("publishSoon(0)")
        seen = wait_for(lambda: LOG.exists() and "Mixdeck E2E radio" in LOG.read_text(encoding="utf-8", errors="ignore"), timeout=20)
        check("Mixdeck app received the tab list", bool(seen))
        if seen:
            # updates stream in; wait for the one that reflects the mute
            def latest():
                lines = [l for l in LOG.read_text(encoding="utf-8", errors="ignore").splitlines() if "Mixdeck E2E radio" in l]
                return lines[-1] if lines and '"muted":true' in lines[-1] else None
            line = wait_for(latest, timeout=15) or "(no muted update)"
            check("tab arrives under the browser's exe", '"browser":"chrome.exe"' in line, line[:160])
            check("tab state is muted at 50 %", '"muted":true' in line and '"volume":0.5' in line, line[:200])

    print(f"\n{'ALL PASSED' if not failures else f'{len(failures)} FAILED'}", flush=True)
    os._exit(1 if failures else 0)
