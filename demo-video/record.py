"""Record the Mixdeck tutorial video headlessly (nothing is shown on screen).

The real UI runs in iframes on a fake desktop (demo-video/stage.html) with the Tauri APIs mocked
(preview/mock-*.ts). Every click and drag goes through the real React components.

    npx vite --config preview/vite.preview.config.ts      # other terminal
    python demo-video/record.py
"""
import os
import shutil
import subprocess
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
OUT = HERE / "out"
URL = "http://localhost:1440/demo-video/stage.html"

# start with Chrome's tabs collapsed, so the video can reveal them
INIT = """try { if (location.search.includes('demo')) localStorage.setItem('mixdeck.collapsed', '{"chrome.exe":true}') } catch {}"""


def main():
    OUT.mkdir(exist_ok=True)
    for f in OUT.glob("*.webm"):
        f.unlink()
    with sync_playwright() as p:
        b = p.chromium.launch(channel="chrome", headless=True)
        ctx = b.new_context(viewport={"width": 1280, "height": 800}, color_scheme="dark",
                            record_video_dir=str(OUT), record_video_size={"width": 1280, "height": 800})
        ctx.add_init_script(INIT)
        page = ctx.new_page()
        t_start = time.time()
        page.goto(URL, wait_until="domcontentloaded", timeout=90000)
        fly = page.frame_locator("#flyout")
        fly.locator(".app").first.wait_for(timeout=90000)
        page.frame_locator("#osd").locator("#root").wait_for(state="attached", timeout=90000)
        page.wait_for_timeout(800)
        skip = max(0.0, time.time() - t_start - 0.4)  # dev-server warm-up: cut it from the video

        def caption(step, text, sub=""):
            page.evaluate("([a, b, c]) => caption(a, b, c)", [step, text, sub])

        def move(x, y, wait=420):
            page.evaluate("([x, y]) => cursorTo(x, y)", [x, y])
            page.mouse.move(x, y, steps=8)
            page.wait_for_timeout(wait)

        def click_at(x, y):
            move(x, y)
            page.evaluate("cursorClick()")
            page.mouse.click(x, y)
            page.wait_for_timeout(250)

        def click(locator):
            bb = locator.bounding_box()
            click_at(bb["x"] + bb["width"] / 2, bb["y"] + bb["height"] / 2)

        def drag(slider, frm, to, steps=24):
            bb = slider.bounding_box()
            y = bb["y"] + bb["height"] / 2
            x0 = bb["x"] + 10 + (bb["width"] - 20) * frm
            x1 = bb["x"] + 10 + (bb["width"] - 20) * to
            move(x0, y)
            page.evaluate("cursorClick()")
            page.mouse.down()
            for i in range(1, steps + 1):
                x = x0 + (x1 - x0) * i / steps
                page.evaluate("([x, y]) => cursorTo(x, y, true)", [x, y])
                page.mouse.move(x, y)
                page.wait_for_timeout(28)
            page.mouse.up()
            page.wait_for_timeout(400)

        def row(name):
            return fly.locator(".app", has_text=name).first

        # ---- 1. tray
        caption("Mixdeck", "A volume mixer that lives in your tray", "Every app, and every browser tab, one click away.")
        move(820, 520, 1500)
        tray = page.locator("#trayicon").bounding_box()
        page.evaluate("document.getElementById('trayicon').classList.add('hot')")
        click_at(tray["x"] + 16, tray["y"] + 16)
        page.evaluate("document.getElementById('flyout').classList.add('show'); document.getElementById('trayicon').classList.remove('hot')")
        page.wait_for_timeout(1400)

        # ---- 2. per-app volume
        caption("Step 1", "Each app gets its own slider", "Live meters show who's making noise. Turn Spotify up…")
        page.wait_for_timeout(1200)
        drag(row("Spotify").locator("input[type=range]"), 0.28, 0.62)
        page.wait_for_timeout(1300)

        # ---- 3. tabs
        caption("Step 2", "Chrome opens up into its tabs", "With the Mixdeck extension, every tab is separate.")
        page.wait_for_timeout(900)
        click(fly.locator(".tabtoggle").first)
        page.wait_for_timeout(1500)

        caption("Step 3", "Quiet the music, keep the call loud", "YouTube down to 15 %, Meet stays at 100 %.")
        page.wait_for_timeout(900)
        drag(fly.locator(".tab", has_text="Lo-fi").locator("input[type=range]"), 0.35, 0.15)
        page.wait_for_timeout(1300)

        caption("Step 4", "Mute a noisy tab in one click", "")
        page.wait_for_timeout(700)
        click(fly.locator(".tab", has_text="Breaking news").locator(".tabicon"))
        page.wait_for_timeout(1600)

        # ---- 4. hotkeys + OSD
        caption("Step 5", "Hotkeys for the app you're using", "Ctrl + Alt + ↑ / ↓ changes the volume of the window in front. No clicking.")
        page.evaluate("document.getElementById('flyout').classList.remove('show')")
        move(400, 330, 300)
        page.evaluate("document.getElementById('music').classList.add('show')")
        page.wait_for_timeout(1300)
        osd = page.frame(url=lambda u: u.endswith("#osd"))
        page.evaluate("document.getElementById('keys').classList.add('show')")
        for v in (0.57, 0.52, 0.47):
            page.evaluate("document.getElementById('k3').classList.add('down')")
            osd.evaluate("(v) => window.__showOsd({ name: 'Spotify', icon: null, volume: v, muted: false, hasAudio: true })", v)
            page.evaluate("document.getElementById('osd').classList.add('show')")
            page.wait_for_timeout(160)
            page.evaluate("document.getElementById('k3').classList.remove('down')")
            page.wait_for_timeout(520)
        page.wait_for_timeout(900)
        page.evaluate("document.getElementById('osd').classList.remove('show'); document.getElementById('keys').classList.remove('show')")
        page.wait_for_timeout(400)

        # ---- 5. memory + settings
        caption("Step 6", "It remembers", "Each app and each site gets its last volume back, even after a restart.")
        page.evaluate("document.getElementById('music').classList.remove('show')")
        click_at(tray["x"] + 16, tray["y"] + 16)
        page.evaluate("document.getElementById('flyout').classList.add('show')")
        page.wait_for_timeout(900)
        click(fly.locator("button[title=Settings]"))
        page.wait_for_timeout(2600)

        # ---- end
        caption("", "")
        page.evaluate("document.getElementById('end').classList.add('show')")
        page.wait_for_timeout(2800)

        video = page.video.path()
        ctx.close()

    webm = OUT / "raw.webm"
    shutil.move(video, webm)
    mp4, gif = OUT / "mixdeck-demo.mp4", OUT / "mixdeck-demo.gif"
    ff = ["ffmpeg", "-y", "-loglevel", "error", "-ss", f"{skip:.2f}"]
    print(f"trimming {skip:.1f} s of warm-up", flush=True)
    subprocess.run(ff + ["-i", str(webm), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "21", "-preset", "fast",
                         "-movflags", "+faststart", str(mp4)], check=True)
    subprocess.run(ff + ["-i", str(webm), "-vf",
                         "fps=12,scale=900:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
                         str(gif)], check=True)
    for f in (mp4, gif):
        print(f"{f}  {f.stat().st_size / 1e6:.1f} MB", flush=True)
    os._exit(0)  # Chrome may hang on close; files are written


if __name__ == "__main__":
    main()
