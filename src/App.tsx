import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AppInfo, EMPTY_MIXER, Levels, Mixer } from "./types";
import { Back, Bars, Chime, Gear, Pin, Speaker } from "./icons";
import "./App.css";

const pct = (v: number) => Math.round(v * 100);
const HOLD_MS = 700; // ignore server echoes for a control the user just touched

/** Live peak meters, animated outside React: elements register by key, a rAF loop writes transforms. */
function useMeters() {
  const els = useRef(new Map<string, HTMLElement>());
  const target = useRef<Levels>({ master: 0, apps: {} });
  const shown = useRef(new Map<string, number>());

  useEffect(() => {
    const un = listen<Levels>("levels", (e) => { target.current = e.payload; });
    let raf = 0;
    const tick = () => {
      els.current.forEach((el, key) => {
        const t = key === "#master" ? target.current.master : target.current.apps[key] ?? 0;
        const prev = shown.current.get(key) ?? 0;
        const v = t > prev ? t : prev * 0.86 + t * 0.14; // fast attack, smooth release
        shown.current.set(key, v);
        // reveal a fixed gradient (like a real VU meter: red only near the top); sqrt ≈ perceptual loudness
        el.style.clipPath = `inset(0 ${(100 - Math.min(1, Math.sqrt(v)) * 100).toFixed(1)}% 0 0)`;
      });
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => { cancelAnimationFrame(raf); un.then((f) => f()); };
  }, []);

  return useCallback((key: string) => (el: HTMLElement | null) => {
    if (el) els.current.set(key, el); else els.current.delete(key);
  }, []);
}

export default function App() {
  const [mixer, setMixer] = useState<Mixer>(EMPTY_MIXER);
  const [local, setLocal] = useState<Record<string, number>>({}); // optimistic slider values
  const touched = useRef<Record<string, number>>({});
  const [view, setView] = useState<"mixer" | "settings">("mixer");
  const [autostart, setAutostart] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const meter = useMeters();

  useEffect(() => {
    invoke<Mixer>("get_mixer").then(setMixer);
    invoke<boolean>("get_autostart").then(setAutostart);
    const a = listen<Mixer>("mixer", (e) => setMixer(e.payload));
    const b = listen("shown", () => setView("mixer"));
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") invoke("hide_window"); };
    window.addEventListener("keydown", onKey);
    return () => { a.then((f) => f()); b.then((f) => f()); window.removeEventListener("keydown", onKey); };
  }, []);

  // Size the native window to the content (bottom-anchored on the Rust side).
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    let last = -1;
    const ro = new ResizeObserver(() => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (Math.abs(h - last) < 1) return; // only real content changes, never echo our own resize
      last = h;
      invoke("fit_window", { height: h });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const valueOf = (key: string, server: number) => {
    const t = touched.current[key];
    return t && Date.now() - t < HOLD_MS && local[key] !== undefined ? local[key] : server;
  };

  const setVol = (key: string, v: number) => {
    v = Math.max(0, Math.min(1, Math.round(v * 100) / 100));
    touched.current[key] = Date.now();
    setLocal((l) => ({ ...l, [key]: v }));
    if (key === "#master") invoke("set_master_volume", { volume: v });
    else invoke("set_volume", { key, volume: v });
  };

  const wheel = (key: string, cur: number) => (e: React.WheelEvent) => setVol(key, cur + (e.deltaY < 0 ? 0.02 : -0.02));

  const master = valueOf("#master", mixer.masterVolume);
  const apps = mixer.apps.filter((a) => !a.system);
  const system = mixer.apps.find((a) => a.system);

  return (
    <div className="flyout" ref={rootRef}>
      {view === "mixer" ? (
        <>
          <header className="head">
            <div className="title">
              <span className="logo" aria-hidden />
              Mixdeck
            </div>
            <select
              className="device"
              value={mixer.followDefault ? "" : mixer.deviceId}
              onChange={(e) => invoke("select_device", { id: e.target.value || null })}
              title="Output device"
            >
              <option value="">Default · {mixer.devices.find((d) => d.isDefault)?.name ?? "none"}</option>
              {mixer.devices.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
            </select>
            <button className="iconbtn" title="Settings" onClick={() => setView("settings")}><Gear /></button>
          </header>

          <section className="master" onWheel={wheel("#master", master)}>
            <button className={`mute ${mixer.masterMuted ? "on" : ""}`} title={mixer.masterMuted ? "Unmute" : "Mute"}
              onClick={() => invoke("set_master_mute", { muted: !mixer.masterMuted })}>
              <Speaker level={master} muted={mixer.masterMuted} />
            </button>
            <div className="body">
              <div className="label">
                <span>Master</span>
                {mixer.masterMuted && <span className="sub">Muted, so every app is silent</span>}
              </div>
              <Slider value={master} muted={mixer.masterMuted} meterRef={meter("#master")} onChange={(v) => setVol("#master", v)} label="Master volume" />
            </div>
            <Pct value={master} onReset={() => setVol("#master", 1)} />
          </section>

          {mixer.error && <div className="error">{mixer.error}</div>}

          <div className="sectiontitle">
            <span>Apps</span>
            <span className="count">{apps.length}</span>
          </div>

          <ul className="apps">
            {apps.length === 0 && !mixer.error && (
              <li className="empty">
                <b>Nothing is using audio yet</b>
                <span>Apps appear here as soon as they open a sound stream.</span>
              </li>
            )}
            {apps.map((a) => (
              <AppRow key={a.key} app={a} value={valueOf(a.key, a.volume)} meterRef={meter(a.key)}
                onChange={(v) => setVol(a.key, v)} onWheel={wheel(a.key, valueOf(a.key, a.volume))} />
            ))}
            {system && (
              <AppRow key={system.key} app={system} value={valueOf(system.key, system.volume)} meterRef={meter(system.key)}
                onChange={(v) => setVol(system.key, v)} onWheel={wheel(system.key, valueOf(system.key, system.volume))} />
            )}
          </ul>

          <footer className="foot">
            <span><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>↑</kbd><kbd>↓</kbd> active app · <kbd>M</kbd> mute</span>
            <button className="link" onClick={() => invoke("open_sound_settings")}>Sound settings</button>
          </footer>
        </>
      ) : (
        <Settings mixer={mixer} autostart={autostart} setAutostart={setAutostart} back={() => setView("mixer")} />
      )}
    </div>
  );
}

function AppRow({ app, value, meterRef, onChange, onWheel }: {
  app: AppInfo; value: number; meterRef: (el: HTMLElement | null) => void; onChange: (v: number) => void; onWheel: (e: React.WheelEvent) => void;
}) {
  return (
    <li className={`app ${app.active ? "playing" : ""} ${app.muted ? "muted" : ""}`} onWheel={onWheel}>
      <button className="appicon" title={app.muted ? `Unmute ${app.name}` : `Mute ${app.name}`}
        onClick={() => invoke("set_mute", { key: app.key, muted: !app.muted })}>
        {app.system ? <span className="sysicon"><Chime /></span>
          : app.icon ? <img src={app.icon} alt="" draggable={false} />
          : <span className="initial">{app.name.slice(0, 1)}</span>}
        {app.muted && <span className="mutedbadge"><Speaker level={0} muted /></span>}
        <span className="hovermute" aria-hidden><Speaker level={app.muted ? 0.5 : 0} muted={!app.muted} /></span>
      </button>
      <div className="body">
        <div className="label">
          <span className="name" title={app.name}>{app.name}</span>
          {app.active && !app.muted && <Bars />}
          {app.remembered && <span className="pin" title="Volume is remembered and restored when the app restarts"><Pin /></span>}
        </div>
        <Slider value={value} muted={app.muted} meterRef={meterRef} onChange={onChange} label={`${app.name} volume`} />
      </div>
      <Pct value={value} onReset={() => onChange(1)} />
    </li>
  );
}

function Slider({ value, muted, meterRef, onChange, label }: {
  value: number; muted: boolean; meterRef: (el: HTMLElement | null) => void; onChange: (v: number) => void; label: string;
}) {
  return (
    <div className={`slider ${muted ? "muted" : ""}`} style={{ "--v": value } as React.CSSProperties}>
      <input type="range" min={0} max={100} step={1} value={pct(value)} aria-label={label}
        onChange={(e) => onChange(Number(e.target.value) / 100)} />
      <div className="meter"><span ref={meterRef} /></div>
    </div>
  );
}

function Pct({ value, onReset }: { value: number; onReset: () => void }) {
  return <button className="pct" title="Double-click to reset to 100%" onDoubleClick={onReset}>{pct(value)}</button>;
}

function Settings({ mixer, autostart, setAutostart, back }: {
  mixer: Mixer; autostart: boolean; setAutostart: (v: boolean) => void; back: () => void;
}) {
  const remembered = mixer.apps.filter((a) => a.remembered);
  return (
    <div className="settings">
      <header className="head">
        <button className="iconbtn" onClick={back} title="Back"><Back /></button>
        <div className="title">Settings</div>
      </header>
      <Toggle label="Start with Windows" hint="Mixdeck waits quietly in the tray" on={autostart}
        set={async (v) => setAutostart(await invoke<boolean>("set_autostart", { on: v }))} />
      <Toggle label="Remember app volumes" hint="Restore each app's volume and mute when it starts again" on={mixer.remember}
        set={(v) => invoke("set_remember", { on: v })} />
      {mixer.remember && remembered.length > 0 && (
        <div className="remembered">
          {remembered.map((a) => (
            <div key={a.key} className="rem">
              {a.icon ? <img src={a.icon} alt="" /> : <span className="initial sm">{a.name[0]}</span>}
              <span>{a.name}</span>
              <span className="muted-text">{pct(a.volume)}%{a.muted ? " · muted" : ""}</span>
              <button className="link" onClick={() => invoke("forget", { key: a.key })}>Forget</button>
            </div>
          ))}
        </div>
      )}
      <div className="keys">
        <div className="keystitle">Shortcuts (work in any app)</div>
        <div><span><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>↑</kbd></span>Active app +5%</div>
        <div><span><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>↓</kbd></span>Active app −5%</div>
        <div><span><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>M</kbd></span>Mute / unmute active app</div>
        <div><span>Mouse wheel</span>Over any row: ±2%</div>
        <div><span>Double-click %</span>Reset to 100%</div>
      </div>
      <footer className="foot">
        <span className="muted-text">Mixdeck 0.1 · MIT</span>
        <button className="link danger" onClick={() => invoke("quit")}>Quit Mixdeck</button>
      </footer>
    </div>
  );
}

function Toggle({ label, hint, on, set }: { label: string; hint: string; on: boolean; set: (v: boolean) => void }) {
  return (
    <label className="toggle">
      <div><div>{label}</div><div className="muted-text">{hint}</div></div>
      <input type="checkbox" checked={on} onChange={(e) => set(e.target.checked)} />
      <span className="switch" aria-hidden />
    </label>
  );
}
