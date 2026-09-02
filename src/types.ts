export interface DisplayAnchor {
  deviceName: string;
  workLeft: number;
  workTop: number;
  workWidth: number;
  workHeight: number;
  dpiX: number;
  dpiY: number;
  effectiveDpiX?: number | null;
  effectiveDpiY?: number | null;
}

export interface LayoutAxis {
  anchor: "start" | "end" | "proportional";
  value: number;
}

export interface FencePlacement {
  groupId: string;
  horizontal: LayoutAxis;
  vertical: LayoutAxis;
  offsetXDip: number;
  offsetYDip: number;
}

export interface DesktopItem {
  name: string;
  path: string;
  isDir: boolean;
  extension: string | null;
  size: number | null;
  modifiedAt: number | null;
}

export interface Fence {
  id: string;
  title: string;
  directory: string;
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
  contentColor: string;
  collapsed: boolean;
  locked: boolean;
  displayAnchor?: DisplayAnchor | null;
  placement?: FencePlacement | null;
  items: DesktopItem[];
}

export interface Preferences {
  titleOpacity: number;
  contentOpacity: number;
  showFenceBorder: boolean;
  fenceBorderOpacity: number;
  iconSize: number;
  defaultFenceWidth: number;
  defaultFenceHeight: number;
  ghostMode: boolean;
  ghostModeTrigger: "automatic" | "hotkey";
  ghostOpacity: number;
  ghostHotkey: string;
  startOnBoot: boolean;
  showHiddenFiles: boolean;
  showFenceTitles: boolean;
  showTrayIcon: boolean;
  desktopMode: boolean;
  desktopContextMenu: boolean;
}

export interface Dashboard {
  fences: Fence[];
  preferences: Preferences;
  desktopPath: string | null;
}

export interface SweepGroup {
  key: string;
  label: string;
  count: number;
  bytes: number;
}

export interface SweepPreview {
  total: number;
  groups: SweepGroup[];
}

export interface NewFenceInput {
  title: string;
  color: string;
  contentColor: string;
  directory?: string;
}
