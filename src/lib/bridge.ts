import { invoke } from "@tauri-apps/api/core";
import type {
  Dashboard,
  Fence,
  NewFenceInput,
  Preferences,
  SweepPreview
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

export async function takePendingNavigation(): Promise<string | null> {
  if (!isTauri()) return null;
  return invokeLogged<string | null>("take_pending_navigation");
}

const browserItems = [
  {
    name: "DCreel 产品草图.fig",
    path: "/demo/DCreel 产品草图.fig",
    isDir: false,
    extension: "fig",
    size: 3_840_000,
    modifiedAt: Date.now()
  },
  {
    name: "需求与灵感",
    path: "/demo/需求与灵感",
    isDir: true,
    extension: null,
    size: null,
    modifiedAt: Date.now()
  }
];

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
      items: browserItems
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
      items: [
        {
          name: "桌面整理方案.pdf",
          path: "/demo/桌面整理方案.pdf",
          isDir: false,
          extension: "pdf",
          size: 1_420_000,
          modifiedAt: Date.now()
        },
        {
          name: "参考截图.png",
          path: "/demo/参考截图.png",
          isDir: false,
          extension: "png",
          size: 840_000,
          modifiedAt: Date.now()
        },
        {
          name: "开发文档.md",
          path: "/demo/开发文档.md",
          isDir: false,
          extension: "md",
          size: 24_000,
          modifiedAt: Date.now()
        }
      ]
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
      items: []
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
    desktopContextMenu: true
  },
  desktopPath: "C:\\Users\\DCreel\\Desktop"
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
    items: []
  };
}

export async function loadDashboard(): Promise<Dashboard> {
  if (!isTauri()) return structuredClone(mock);
  return invokeLogged<Dashboard>("load_dashboard");
}

export async function setDesktopVisibility(visible: boolean): Promise<boolean> {
  if (!isTauri()) return visible;
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
    const fence = mockFence(input, input.directory ?? "C:\\Users\\DCreel\\Documents");
    mock.fences.push(fence);
    return structuredClone(fence);
  }
  return invokeLogged<Fence>("create_mapped_box", { input });
}

export async function saveFence(fence: Fence): Promise<void> {
  if (!isTauri()) {
    mock.fences = mock.fences.map((current) =>
      current.id === fence.id ? structuredClone(fence) : current
    );
    return;
  }
  const { items: _items, ...payload } = fence;
  await invokeLogged("update_fence", { fence: payload });
}

export async function deleteFence(id: string): Promise<void> {
  if (!isTauri()) {
    mock.fences = mock.fences.filter((fence) => fence.id !== id);
    return;
  }
  await invokeLogged("remove_fence", { id });
}

export async function savePreferences(preferences: Preferences): Promise<Preferences> {
  if (!isTauri()) {
    mock.preferences = structuredClone(preferences);
    return preferences;
  }
  return invokeLogged<Preferences>("update_preferences", { preferences });
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

export async function revealItem(path: string): Promise<void> {
  if (!isTauri()) return;
  await invokeLogged("reveal_path", { path });
}

export async function getSweepPreview(): Promise<SweepPreview> {
  if (!isTauri()) {
    return {
      total: 23,
      groups: [
        { key: "images", label: "图片素材", count: 8, bytes: 18_400_000 },
        { key: "documents", label: "文档资料", count: 7, bytes: 5_200_000 },
        { key: "archives", label: "压缩包", count: 3, bytes: 42_000_000 },
        { key: "shortcuts", label: "应用快捷方式", count: 5, bytes: 92_000 }
      ]
    };
  }
  return invokeLogged<SweepPreview>("preview_desktop_sweep");
}

export async function runSweep(): Promise<Dashboard> {
  if (!isTauri()) return structuredClone(mock);
  return invokeLogged<Dashboard>("organize_desktop");
}
