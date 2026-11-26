"""Behaviour tests for page-gain.js in a real (headless) Chrome.

    python extension/tests/test_page_gain.py
"""
import os
from pathlib import Path

from playwright.sync_api import sync_playwright

SCRIPT = (Path(__file__).resolve().parent.parent / "page-gain.js").read_text(encoding="utf-8")
# Keep a handle on the browser's real getter, before page-gain.js patches it.
CAPTURE = "{ const g = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'volume').get; window.__realVolume = (el) => g.call(el); }"

PAGE = "<html><body><audio id=a></audio></body></html>"


def gain(page, g):
    page.evaluate("(g) => document.dispatchEvent(new CustomEvent('mixdeck:gain', { detail: g }))", g)


def run():
    failures = []

    def check(name, cond, detail=""):
        print(("PASS " if cond else "FAIL ") + name + (f"  ({detail})" if detail else ""))
        if not cond:
            failures.append(name)

    with sync_playwright() as p:
        b = p.chromium.launch(channel="chrome", headless=True, args=["--autoplay-policy=no-user-gesture-required"])
        page = b.new_page()
        page.add_init_script(CAPTURE)
        page.add_init_script(SCRIPT)
        page.route("http://mixdeck.test/", lambda r: r.fulfill(body=PAGE, content_type="text/html"))
        page.goto("http://mixdeck.test/")  # a real navigation, so the init scripts run

        # site sets 0.8, Mixdeck gain 0.5 → real 0.4, site still reads 0.8
        page.evaluate("document.getElementById('a').volume = 0.8")
        gain(page, 0.5)
        real = page.evaluate("__realVolume(document.getElementById('a'))")
        seen = page.evaluate("document.getElementById('a').volume")
        check("element volume is multiplied", abs(real - 0.4) < 1e-6, f"real={real}")
        check("site still reads its own volume", abs(seen - 0.8) < 1e-6, f"seen={seen}")

        # site changes volume after the gain was set
        page.evaluate("document.getElementById('a').volume = 0.6")
        real = page.evaluate("__realVolume(document.getElementById('a'))")
        check("later site changes keep the gain", abs(real - 0.3) < 1e-6, f"real={real}")

        # detached new Audio() caught at play()
        # (don't return play()'s promise: with no source it never settles)
        page.evaluate("window.x = new Audio(); x.volume = 1; void x.play().catch(() => {})")
        real = page.evaluate("__realVolume(x)")
        check("detached Audio() gets the gain", abs(real - 0.5) < 1e-6, f"real={real}")

        # gain back to 1 restores the exact site volume
        gain(page, 1)
        real = page.evaluate("__realVolume(document.getElementById('a'))")
        check("gain 1 restores site volume", abs(real - 0.6) < 1e-6, f"real={real}")

        # invalid values still throw like the browser does
        threw = page.evaluate("(() => { try { document.getElementById('a').volume = 2; return false } catch (e) { return e.name } })()")
        check("out-of-range volume still throws", threw == "IndexSizeError", f"threw={threw}")

        # Web Audio: destination is a GainNode that follows the tab gain
        gain(page, 0.25)
        res = page.evaluate("""async () => {
            const ctx = new AudioContext();
            const d = ctx.destination;
            await new Promise(r => setTimeout(r, 200));
            return { isGain: d instanceof GainNode, same: d === ctx.destination, gain: d.gain.value,
                     maxCh: d.maxChannelCount, canConnect: (() => { try { new OscillatorNode(ctx).connect(d); return true } catch { return false } })() };
        }""")
        check("Web Audio destination is wrapped", res["isGain"] and res["same"], str(res))
        check("Web Audio gain applied", abs(res["gain"] - 0.25) < 1e-3, str(res["gain"]))
        check("wrapped destination stays usable", res["canConnect"] and res["maxCh"] >= 2, str(res))

        gain(page, 0.75)
        page.wait_for_timeout(250)
        g = page.evaluate("new AudioContext().destination.gain.value")
        check("new contexts start at the current gain", abs(g - 0.75) < 1e-3, str(g))

        # garbage is ignored
        gain(page, "loud")
        gain(page, 5)
        real = page.evaluate("__realVolume(document.getElementById('a'))")
        check("invalid gains are ignored", abs(real - 0.45) < 1e-6, f"real={real}")

        print(f"\n{'ALL PASSED' if not failures else f'{len(failures)} FAILED'}", flush=True)
        # Chrome can hang on close while AudioContexts are alive; the results are in, so leave hard.
        os._exit(1 if failures else 0)


if __name__ == "__main__":
    run()
