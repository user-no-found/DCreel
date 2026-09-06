import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  Dashboard,
  DesktopNotificationPayload,
  Fence,
  FencePatch,
  NewFenceInput,
  Preferences,
  TransferSnapshot
} from "../types";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export const isTauri = () => Boolean(window.__TAURI_INTERNALS__);
export const PROJECT_REPOSITORY_URL = "https://github.com/user-no-found/DCreel";

function bridgeErrorMessage(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

async function invokeLogged<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    void invoke("write_frontend_log", {
      level: "error",
      message: `IPC command ${command} failed: ${bridgeErrorMessage(error)}`,
      source: "bridge"
    }).catch(() => undefined);
    throw error;
  }
}

export async function writeFrontendLog(
  level: "debug" | "info" | "warn" | "error",
  message: string,
  source = "webview"
): Promise<void> {
  if (!isTauri()) return;
  await invoke("write_frontend_log", { level, message, source });
}

export async function completeStartup(): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("complete_startup");
}

export async function waitForBackendReady(timeoutMs = 12_000): Promise<void> {
  if (!isTauri()) return;
  const deadline = performance.now() + timeoutMs;
  let lastError: unknown;
  while (performance.now() < deadline) {
    try {
      if (await invoke<boolean>("backend_ready")) return;
    } catch (error) {
      lastError = error;
    }
    await new Promise<void>((resolve) => window.setTimeout(resolve, 50));
  }
  const detail = lastError ? `：${bridgeErrorMessage(lastError)}` : "";
  throw new Error(`DCreel 后端在 ${Math.round(timeoutMs / 1000)} 秒内没有就绪${detail}`);
}

export async function openLogDirectory(): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("open_log_directory");
}

export async function quitApplication(): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("quit_app");
}

export async function takePendingNavigation(): Promise<string | null> {
  if (!isTauri()) return null;
  return invokeLogged<string | null>("take_pending_navigation");
}

const browserDashboard: Dashboard = {
  fences: [
    {
      id: "inbox",
      title: "今日待处理",
      directory: "C:\\Users\\DCreel\\Desktop\\今日待处理",
      x: 34,
      y: 38,
      width: 330,
      height: 285,
      color: "coral",
      contentColor: "paper",
      collapsed: false,
      locked: false,
      itemCount: 2
    },
    {
      id: "projects",
      title: "项目资料",
      directory: "C:\\Users\\DCreel\\Desktop\\项目资料",
      x: 392,
      y: 82,
      width: 360,
      height: 330,
      color: "sage",
      contentColor: "frosted",
      collapsed: false,
      locked: false,
      itemCount: 3
    },
    {
      id: "inspiration",
      title: "灵感素材",
      directory: "C:\\Users\\DCreel\\Desktop\\灵感素材",
      x: 782,
      y: 48,
      width: 292,
      height: 245,
      color: "butter",
      contentColor: "paper",
      collapsed: false,
      locked: false,
      itemCount: 0
    }
  ],
  preferences: {
    titleOpacity: 0.9,
    contentOpacity: 0.9,
    showFenceBorder: true,
    fenceBorderOpacity: 0.4,
    iconSize: 46,
    defaultFenceWidth: 330,
    defaultFenceHeight: 280,
    ghostMode: false,
    ghostModeTrigger: "automatic",
    ghostOpacity: 0.2,
    ghostHotkey: "Ctrl+Alt+G",
    startOnBoot: false,
    showHiddenFiles: false,
    showFenceTitles: true,
    showTrayIcon: true,
    desktopMode: true,
    desktopContextMenu: true,
    ignoredUpdateVersion: null
  },
  desktopVisible: true
};

let mock = structuredClone(browserDashboard);

function mockFence(input: NewFenceInput, directory: string): Fence {
  const offset = mock.fences.length * 26;
  return {
    id: crypto.randomUUID(),
    title: input.title,
    directory,
    x: 56 + (offset % 420),
    y: 56 + (offset % 180),
    width: mock.preferences.defaultFenceWidth,
    height: mock.preferences.defaultFenceHeight,
    color: input.color,
    contentColor: input.contentColor,
    collapsed: false,
    locked: false,
    itemCount: 0
  };
}

export async function loadDashboard(): Promise<Dashboard> {
  if (!isTauri()) return structuredClone(mock);
  return invokeLogged<Dashboard>("load_dashboard");
}

export async function setDesktopVisibility(visible: boolean): Promise<boolean> {
  if (!isTauri()) {
    mock.desktopVisible = visible;
    return visible;
  }
  return invokeLogged<boolean>("set_desktop_visibility", { visible });
}

export async function createStorageBox(input: NewFenceInput): Promise<Fence> {
  if (!isTauri()) {
    const parent = input.directory ?? "C:\\Users\\DCreel\\Documents";
    const fence = mockFence(input, `${parent}\\${input.title}`);
    mock.fences.push(fence);
    return structuredClone(fence);
  }
  return invokeLogged<Fence>("create_storage_box", { input });
}

export async function createMappedBox(input: NewFenceInput): Promise<Fence> {
  if (!isTauri()) {
    const directory = input.directory ?? "C:\\Users\\DCreel\\Documents";
    const existing = mock.fences.find(
      (fence) => fence.directory.toLocaleLowerCase() === directory.toLocaleLowerCase()
    );
    if (existing) throw new Error(`该文件夹已经映射为「${existing.title}」`);
    const fence = mockFence(input, directory);
    mock.fences.push(fence);
    return structuredClone(fence);
  }
  return invokeLogged<Fence>("create_mapped_box", { input });
}

export async function saveFence(id: string, patch: FencePatch): Promise<Fence> {
  if (!isTauri()) {
    mock.fences = mock.fences.map((current) =>
      current.id === id ? { ...current, ...structuredClone(patch) } : current
    );
    const updated = mock.fences.find((fence) => fence.id === id);
    if (!updated) throw new Error(`没有找到盒子：${id}`);
    return structuredClone(updated);
  }
  return invokeLogged<Fence>("update_fence", { id, patch });
}

export async function deleteFence(id: string): Promise<void> {
  if (!isTauri()) {
    mock.fences = mock.fences.filter((fence) => fence.id !== id);
    return;
  }
  await invokeLogged("remove_fence", { id });
}

export async function savePreferences(patch: Partial<Preferences>): Promise<Preferences> {
  if (!isTauri()) {
    mock.preferences = { ...mock.preferences, ...structuredClone(patch) };
    return structuredClone(mock.preferences);
  }
  return invokeLogged<Preferences>("update_preferences", { patch });
}

export async function openItem(path: string): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("open_path", { path });
}

export async function openProjectRepository(): Promise<void> {
  if (!isTauri()) {
    window.open(PROJECT_REPOSITORY_URL, "_blank", "noopener,noreferrer");
    return;
  }
  await invokeLogged("open_project_repository");
}

export async function setHotkeyCaptureActive(active: boolean): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("set_hotkey_capture_active", { active });
}

export async function ignoreUpdateVersion(version: string): Promise<void> {
  if (!isTauri()) {
    mock.preferences.ignoredUpdateVersion = version;
    return;
  }
  await invokeLogged("ignore_update_version", { version });
}

export async function showUpdateNotification(version: string): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("show_update_notification", { version });
}

export async function showUpdateDetails(notificationId: string): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("show_update_details", { notificationId });
}

export async function cancelFileTransfer(id: string): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("cancel_file_transfer", { id });
}

export async function currentFileTransfer(): Promise<TransferSnapshot | null> {
  if (!isTauri()) return null;
  return invokeLogged<TransferSnapshot | null>("current_file_transfer");
}

export async function dismissAuxiliaryWindow(notificationId?: string): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("dismiss_auxiliary_window", { notificationId });
}

export async function subscribeDesktopNotifications(onNotification: (notification: DesktopNotificationPayload) => void): Promise<void> {
  if (!isTauri()) return;
  const onNotificationChannel = new Channel<DesktopNotificationPayload>();
  onNotificationChannel.onmessage = onNotification;
  await invokeLogged("subscribe_desktop_notifications", { onNotification: onNotificationChannel });
}

export async function presentDesktopNotification(id: string): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("present_desktop_notification", { id });
}
