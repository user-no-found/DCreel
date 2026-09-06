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
  itemCount: number;
}

export type FencePatch = Partial<
  Pick<Fence, "title" | "color" | "contentColor" | "collapsed" | "locked">
>;

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
  ignoredUpdateVersion?: string | null;
}

export type TransferPhase =
  | "queued"
  | "preparing"
  | "moving"
  | "finalizing"
  | "rolling_back"
  | "completed"
  | "cancelled"
  | "failed";

export interface TransferSnapshot {
  id: string;
  fenceTitle: string;
  phase: TransferPhase;
  totalBytes: number;
  completedBytes: number;
  totalItems: number;
  completedItems: number;
  currentItem?: string | null;
  message?: string | null;
  canCancel: boolean;
}

export interface DesktopNotificationPayload {
  id: string;
  kind: "message" | "update";
  title: string;
  message: string;
  version?: string;
}

export interface Dashboard {
  fences: Fence[];
  preferences: Preferences;
  desktopVisible: boolean;
}

export interface NewFenceInput {
  title: string;
  color: string;
  contentColor: string;
  directory?: string;
}
