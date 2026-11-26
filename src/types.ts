export interface AppInfo {
  key: string;
  name: string;
  icon: string | null;
  volume: number;
  muted: boolean;
  active: boolean;
  system: boolean;
  remembered: boolean;
  sessions: number;
}

export interface Device {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface Mixer {
  devices: Device[];
  deviceId: string;
  followDefault: boolean;
  masterVolume: number;
  masterMuted: boolean;
  apps: AppInfo[];
  remember: boolean;
  error: string | null;
}

export interface Levels {
  master: number;
  apps: Record<string, number>;
}

export interface Osd {
  name: string;
  icon: string | null;
  volume: number;
  muted: boolean;
  hasAudio: boolean;
}

export interface Tab {
  id: number;
  title: string;
  host: string;
  favIconUrl: string;
  audible: boolean;
  muted: boolean;
  volume: number;
  active: boolean;
}

export interface BrowserTabs {
  conn: number;
  browser: string; // executable key, matches AppInfo.key
  name: string;
  tabs: Tab[];
}

export interface BrowserStatus {
  registered: boolean;
  error: string | null;
  extensionId: string;
}

export const EMPTY_MIXER: Mixer = {
  devices: [], deviceId: "", followDefault: true, masterVolume: 0, masterMuted: false, apps: [], remember: true, error: null,
};
