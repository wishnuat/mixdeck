import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Osd as OsdData } from "./types";
import { Speaker } from "./icons";

export default function Osd() {
  const [d, setD] = useState<OsdData | null>(null);
  const [bump, setBump] = useState(0);
  useEffect(() => {
    const un = listen<OsdData>("osd", (e) => { setD(e.payload); setBump((b) => b + 1); });
    return () => { un.then((f) => f()); };
  }, []);
  if (!d) return null;
  const v = Math.round(d.volume * 100);
  return (
    <div className="osd" key={bump}>
      {d.icon ? <img src={d.icon} alt="" /> : <span className="initial">{d.name.slice(0, 1)}</span>}
      <div className="osdbody">
        <div className="osdname">{d.name}</div>
        {d.hasAudio ? (
          <div className="osdbar"><span style={{ width: `${d.muted ? 0 : v}%` }} /></div>
        ) : (
          <div className="osdnote">isn't playing any audio</div>
        )}
      </div>
      {d.hasAudio && (
        <div className="osdval">{d.muted ? <Speaker level={0} muted /> : `${v}`}</div>
      )}
    </div>
  );
}
