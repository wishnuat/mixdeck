# Mixdeck: Product Plan

> One mixer for every sound on your PC: each app, and (phase 2) each browser tab.

## Problem

Windows already has a per-app Volume Mixer, but:

1. **It's buried.** It takes three clicks into Settings, so nobody adjusts a single app on the fly.
2. **It forgets.** Many apps (Chrome, Discord, games) start a new audio session on launch and come back at 100%.
3. **The browser is one blob.** All tabs share one audio process, so Windows can't tell a YouTube tab from a Meet
   call. Per-tab extensions exist, but they live in a different place from the app mixer.
4. **It's slow to reach.** Turning down *the app you're looking at* means opening a panel and hunting for its row.

## Who it's for

| Persona | Moment |
|---|---|
| Remote worker | Meet/Zoom must stay loud while Spotify and a YouTube tutorial play quietly |
| Gamer / streamer | Game at 60 %, Discord at 100 %, browser music at 20 %, the same every session |
| Office user | A noisy ad tab or a notification-heavy app needs muting *now* |

## MVP scope (phase 1: this release)

| # | Capability | Acceptance criteria |
|---|---|---|
| 1 | Tray flyout mixer | Left-click on the tray opens a Windows 11-style flyout (Mica/Acrylic, rounded) anchored above the tray. It closes when focus is lost. It opens in < 150 ms |
| 2 | Per-app volume & mute | Every app with an audio session appears once (sessions grouped by executable), with its real icon and product name. Slider changes apply live |
| 3 | Live level meters | Each app and the master show a real-time peak meter (~30 fps) while the flyout is open. Polling stops while it's hidden (no idle CPU cost) |
| 4 | Remembered volumes | When an app (re)starts, or opens a new session such as a new Chrome audio stream, its last volume and mute state are restored automatically. Can be switched off |
| 5 | Foreground hotkeys | `Ctrl+Alt+↑/↓` changes the volume of *the app in the foreground* by 5 %, and `Ctrl+Alt+M` mutes it. An on-screen display shows the app icon, name and level |
| 6 | Output devices | The device picker shows the sessions and master volume of any active output. It follows the Windows default automatically |
| 7 | Start with Windows | Toggle in settings (registry Run key via the autostart plugin) |
| 8 | Keyboard and wheel | Mouse wheel over a row changes it by ±2 %. Arrow keys work on a focused slider. Double-click the % to reset to 100 % |

### Non-goals for the MVP
Changing the *default* output device (needs an undocumented COM interface), per-app output routing, EQ/effects,
macOS/Linux, and per-tab control (phase 2).

## Phase 2: browser tabs (shipped in 0.2)

A Chrome/Edge extension (MV3) plus a native-messaging host built into Mixdeck. The Chrome row becomes expandable into
its audible tabs, each with volume and mute.
- Default path: control `<audio>`/`<video>` elements from a content script (0–100 %, no permission prompt per tab)
- Boost path: `chrome.tabCapture` and a WebAudio gain node for > 100 % (Chrome requires one click per tab)

## Architecture

```
┌──────────── Tauri (Rust) ─────────────────────────────────────────┐
│ audio thread (COM MTA, owns all WASAPI objects)                    │
│   IMMDeviceEnumerator → IAudioSessionManager2 → sessions           │
│   ISimpleAudioVolume · IAudioMeterInformation · IAudioEndpointVolume│
│   ▲ commands (mpsc)            │ events: "mixer" 1 Hz / on change  │
│   │                            │         "levels" 30 Hz if visible │
│ tray · global shortcuts · OSD window · settings.json               │
└───────────────┬────────────────┴───────────────────────────────────┘
                │ invoke / listen
        React flyout (index.html)   ·   OSD (index.html#osd)
```

All COM objects live on one dedicated thread, and the rest of the app talks to it through a channel. This avoids
`!Send` interface types and apartment issues, and gives one place to throttle polling.

## Risks

| Risk | Mitigation |
|---|---|
| Apps that create a new session per stream (Chrome) | Group by executable path. New sessions in a known group inherit its volume |
| Protected processes (no `OpenProcess`) | Fall back to the session display name and a generic icon |
| Idle CPU from meters | Meters are only polled while the flyout or OSD is visible |
| Elevated apps vs global hotkeys | Documented limitation. Hotkeys don't reach admin windows unless Mixdeck runs elevated |

## Milestones

1. Engine: sessions, volume, mute, meters, device list (Rust, testable via a CLI example)
2. Flyout UI: tray, Mica, rows, meters, master, device picker
3. Memory: remembered volumes and new-session inheritance
4. Hotkeys and OSD
5. Polish: autostart, settings, empty and error states, README, release
6. Phase 2: browser extension, done: per-tab volume and mute, per-site memory, standalone popup, E2E-tested over native messaging
