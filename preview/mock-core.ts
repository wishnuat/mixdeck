// Browser-only stand-in for @tauri-apps/api/core (UI review + screenshots).
import { emitMock, MIXER } from "./mock-state";

export async function invoke(cmd: string, args?: any): Promise<any> {
  switch (cmd) {
    case "get_mixer": return structuredClone(MIXER);
    case "get_autostart": return true;
    case "set_autostart": return args.on;
    case "set_volume": {
      const a = MIXER.apps.find((x) => x.key === args.key);
      if (a) { a.volume = args.volume; a.muted = false; }
      emitMock("mixer", structuredClone(MIXER));
      return;
    }
    case "set_mute": {
      const a = MIXER.apps.find((x) => x.key === args.key);
      if (a) a.muted = args.muted;
      emitMock("mixer", structuredClone(MIXER));
      return;
    }
    case "set_master_volume": MIXER.masterVolume = args.volume; emitMock("mixer", structuredClone(MIXER)); return;
    case "set_master_mute": MIXER.masterMuted = args.muted; emitMock("mixer", structuredClone(MIXER)); return;
    default: return;
  }
}
