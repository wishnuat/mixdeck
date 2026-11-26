"""Render the flyout/OSD headlessly over a wallpaper-like backdrop and save screenshots.

    npx vite --config preview/vite.preview.config.ts      # other terminal
    python preview/shoot.py
"""
from pathlib import Path
from playwright.sync_api import sync_playwright

OUT = Path(__file__).resolve().parent / "out"

# Fake desktop behind the transparent window, to approximate Mica tinting.
BACKDROP = """(dark) => {
  document.documentElement.style.background = dark
    ? 'radial-gradient(circle at 20% 10%, #23395b, #121218 60%)'
    : 'radial-gradient(circle at 20% 10%, #cfe3ff, #f1eef7 60%)';
}"""


def main():
    OUT.mkdir(exist_ok=True)
    with sync_playwright() as p:
        b = p.chromium.launch(channel="chrome", headless=True)
        for scheme in ("dark", "light"):
            ctx = b.new_context(viewport={"width": 380, "height": 560}, color_scheme=scheme, device_scale_factor=2)
            page = ctx.new_page()
            page.goto("http://localhost:1440/")
            page.evaluate(BACKDROP, scheme == "dark")
            page.wait_for_selector(".app")
            page.wait_for_timeout(900)
            h = page.evaluate("Math.ceil(document.querySelector('.flyout').getBoundingClientRect().height)")
            page.screenshot(path=str(OUT / f"flyout-{scheme}.png"), clip={"x": 0, "y": 0, "width": 380, "height": h})
            page.click("button[title=Settings]")
            page.wait_for_timeout(300)
            h = page.evaluate("Math.ceil(document.querySelector('.flyout').getBoundingClientRect().height)")
            page.screenshot(path=str(OUT / f"settings-{scheme}.png"), clip={"x": 0, "y": 0, "width": 380, "height": h})

            page.close()
            octx = b.new_context(viewport={"width": 300, "height": 72}, color_scheme=scheme, device_scale_factor=2)
            osd = octx.new_page()
            osd.goto("http://localhost:1440/#osd", wait_until="domcontentloaded")
            osd.evaluate(BACKDROP, scheme == "dark")
            osd.wait_for_timeout(300)
            osd.evaluate("window.__showOsd({name:'Spotify', icon:null, volume:0.35, muted:false, hasAudio:true})")
            osd.wait_for_timeout(400)
            osd.screenshot(path=str(OUT / f"osd-{scheme}.png"))
            octx.close()
            ctx.close()
        b.close()
    print("saved to", OUT)


if __name__ == "__main__":
    main()
