# Mixdeck

[![CI](https://github.com/wishnuat/mixdeck/actions/workflows/ci.yml/badge.svg)](https://github.com/wishnuat/mixdeck/actions/workflows/ci.yml)

**A tray mixer for Windows 11: volume per app with live level meters, remembered volumes, and hotkeys for the app
you're using. Browser-tab control is next.**

<p>
  <img src="docs/flyout-dark.png" width="300" alt="Mixdeck flyout, dark theme" />
  <img src="docs/flyout-light.png" width="300" alt="Mixdeck flyout, light theme" />
</p>

Windows already has a per-app volume mixer, but it's three clicks deep in Settings, it forgets your levels when an
app restarts, and it can't tell one browser tab from another. Mixdeck lives in the tray and fixes the first two now;
the third is [phase 2](docs/PLAN.md#phase-2-browser-tabs).

## Features

- **Flyout from the tray**, anchored above the clock like the built-in flyouts, with a Mica backdrop, rounded
  corners, and light and dark themes. It closes when you click elsewhere
- **Volume and mute for every app**, with real icons and product names read from each executable. Apps that open
  many streams (Chrome, Discord) still show as **one row**
- **Live peak meters** for every app and the master output, so you can see *who* is making that sound. Meters run
  only while the flyout is open, so there's no idle CPU cost
- **Remembered volumes.** When an app restarts, or opens a new audio stream (the reason Chrome keeps coming back at
  100 %), its last volume and mute state are restored automatically. You can manage or forget them in Settings
- **Hotkeys for the app in front**, from any app: `Ctrl+Alt+↑/↓` changes its volume by 5 % and `Ctrl+Alt+M`
  mutes it. A small on-screen display confirms the change

  <img src="docs/osd-dark.png" width="300" alt="On-screen display" />

- **Output devices.** Pick any active output, or follow the Windows default (switching headphones is picked up
  automatically)
- Mouse wheel over a row changes it by ±2 %, double-clicking the percentage resets it to 100 %, and `Esc` closes the
  flyout
- Start with Windows, single instance, and about a 3 MB installer

## How it works

```
┌──────────── Rust (Tauri) ─────────────────────────────────────────┐
│ audio thread: one COM MTA thread owns every WASAPI object          │
│   IMMDeviceEnumerator → IAudioSessionManager2 → sessions           │
│   ISimpleAudioVolume · IAudioMeterInformation · IAudioEndpointVolume│
│   ▲ Cmd channel                  │ "mixer" on change (1 Hz scan)   │
│   │                              │ "levels" 30 Hz while visible    │
│ tray · global shortcuts · OSD window · mixdeck.json                │
└──────────────┬───────────────────┴─────────────────────────────────┘
               │ invoke / listen
      React flyout (index.html)   ·   OSD (index.html#osd)
```

- **One thread owns COM.** WASAPI interfaces are `!Send`, so a dedicated thread holds them and the UI, tray and
  hotkeys send it commands over a channel. Bursts of slider events are coalesced before they're applied
- **Sessions are grouped by executable.** Every scan also checks for *new* session instance IDs, which receive the
  remembered volume, or the volume of an existing sibling session from the same app
- **Meters don't re-render React.** Peaks arrive at 30 Hz; a `requestAnimationFrame` loop writes `clip-path`
  directly to the DOM with fast attack, smooth release and a square-root curve for perceived loudness
- **The slider is optimistic.** For 700 ms after you touch a control, server snapshots for that control are ignored,
  so the thumb never jumps back while you drag
- **Product names and icons** come from the executable's version resource (`FileDescription`) and
  `PrivateExtractIconsW`, converted from BGRA to PNG data URLs

## Development

Requirements: Windows 10/11, Rust (MSVC), Node 18+.

```bash
npm install
npm run tauri dev            # run the app
npm run tauri build          # installer → src-tauri/target/release/bundle/nsis/

cd src-tauri
cargo test                   # unit tests + a live test against the machine's audio stack
cargo run --example snapshot # print devices, apps and peak levels as the engine sees them
```

`preview/` renders the real UI in a browser with the Tauri APIs mocked, for UI review and the screenshots above:

```bash
npx vite --config preview/vite.preview.config.ts
python preview/shoot.py
```

## Roadmap

See [docs/PLAN.md](docs/PLAN.md) for the full product plan.

- [x] Phase 1: per-app mixer, meters, memory, hotkeys, OSD, devices
- [ ] Phase 2: Chrome/Edge extension, so the browser row expands into its tabs (native messaging)
- [ ] Per-app output device routing
- [ ] Profiles ("Meeting", "Gaming") switchable from the tray

## License

MIT
