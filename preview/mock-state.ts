// Fake but realistic mixer state for UI review. Icons are simple generated tiles (no third-party logos).
import type { Mixer } from "../src/types";

const tile = (bg: string, letter: string) =>
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="${bg}"/>` +
      `<text x="32" y="43" font-family="Segoe UI,Arial" font-size="32" font-weight="700" text-anchor="middle" fill="#fff">${letter}</text></svg>`,
  );

export const MIXER: Mixer = {
  devices: [
    { id: "spk", name: "Speakers (Realtek(R) Audio)", isDefault: true },
    { id: "hp", name: "Headphones (WH-1000XM5)", isDefault: false },
  ],
  deviceId: "spk",
  followDefault: true,
  masterVolume: 0.62,
  masterMuted: false,
  remember: true,
  error: null,
  apps: [
    { key: "chrome.exe", name: "Google Chrome", icon: tile("#1a73e8", "C"), volume: 0.45, muted: false, active: true, system: false, remembered: true, sessions: 3 },
    { key: "discord.exe", name: "Discord", icon: tile("#5865f2", "D"), volume: 1, muted: false, active: true, system: false, remembered: true, sessions: 1 },
    { key: "spotify.exe", name: "Spotify", icon: tile("#1db954", "S"), volume: 0.28, muted: false, active: true, system: false, remembered: true, sessions: 1 },
    { key: "teams.exe", name: "Microsoft Teams", icon: tile("#5b5fc7", "T"), volume: 0.8, muted: true, active: false, system: false, remembered: false, sessions: 1 },
    { key: "#system", name: "System sounds", icon: null, volume: 0.5, muted: false, active: false, system: true, remembered: false, sessions: 1 },
  ],
};

const subs = new Map<string, Set<(p: unknown) => void>>();
export function subscribe(event: string, cb: (p: unknown) => void) {
  if (!subs.has(event)) subs.set(event, new Set());
  subs.get(event)!.add(cb);
  return () => subs.get(event)!.delete(cb);
}
export function emitMock(event: string, payload: unknown) {
  subs.get(event)?.forEach((cb) => cb(payload));
}

// Fake peak meters: music-like movement for the playing apps.
let t = 0;
setInterval(() => {
  t += 1;
  const wob = (f: number, p: number) => Math.max(0, 0.55 + 0.4 * Math.sin(t / f + p) * Math.sin(t / (f * 2.7) + p * 2));
  emitMock("levels", {
    master: wob(3, 0.3) * 0.8,
    apps: { "chrome.exe": wob(4, 1) * 0.6, "discord.exe": t % 40 < 18 ? wob(2, 2) : 0.02, "spotify.exe": wob(3.3, 0.5) },
  });
}, 33);

(window as any).__showOsd = (o: unknown) => emitMock("osd", o);
