import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent
} from "react";
import {
  AppWindow,
  Boxes,
  Check,
  ChevronDown,
  ChevronRight,
  CircleCheck,
  CircleHelp,
  Copy,
  Download,
  FolderInput,
  FolderOpen,
  Eye,
  EyeOff,
  ExternalLink,
  Inbox,
  GitFork,
  LayoutGrid,
  Link2,
  Lock,
  Minus,
  MonitorUp,
  Palette,
  Pencil,
  Power,
  Plus,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  Square,
  Trash2,
  Unlock,
  X
} from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  createMappedBox,
  createStorageBox,
  completeStartup,
  deleteFence,
  isTauri,
  loadDashboard,
  openItem,
  openLogDirectory,
  openProjectRepository,
  PROJECT_REPOSITORY_URL,
  quitApplication,
  saveFence,
  savePreferences,
  setDesktopVisibility,
  setHotkeyCaptureActive,
  takePendingNavigation,
  waitForBackendReady,
  writeFrontendLog
} from "./lib/bridge";
import { basename, formatBytes } from "./lib/format";
import { clearMatchingPatch, mergeDashboardWithOptimistic } from "./lib/stateSync";
import type { Dashboard, Fence, FencePatch, Preferences } from "./types";

type View = "desktop" | "settings" | "about";
type Modal = "new" | null;
type NewFenceKind = "storage" | "mapped";
type UpdaterPhase =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "downloaded"
  | "installing"
  | "error";

interface UpdaterState {
  phase: UpdaterPhase;
  availableVersion?: string;
  releaseNotes?: string;
  downloadedBytes: number;
  totalBytes?: number;
  error?: string;
}

const initialUpdaterState: UpdaterState = {
  phase: "idle",
  downloadedBytes: 0
};

const colors = ["coral", "sage", "butter", "sky", "lilac", "graphite"];
const colorValues: Record<string, string> = {
  coral: "#df765d",
  sage: "#8ca88d",
  butter: "#dcbf67",
  sky: "#7fa7b4",
  lilac: "#a28eae",
  graphite: "#747471",
  paper: "#f5efe5",
  frosted: "#e5eef1"
};

function resolvedColor(value: string): string {
  return colorValues[value] ?? (/^#[0-9a-f]{6}$/i.test(value) ? value : colorValues.coral);
}

function customColorValue(value: string, fallback: string): string {
  return /^#[0-9a-f]{6}$/i.test(value) ? value : resolvedColor(value || fallback);
}
const viewMeta: Record<View, { eyebrow: string; title: string; summary: string }> = {
  desktop: {
    eyebrow: "DESKTOP BOXES",
    title: "桌面盒子",
    summary: "在桌面上查看和整理你选择的本地文件夹"
  },
  settings: {
    eyebrow: "PREFERENCES",
    title: "DCreel 设置",
    summary: "调整桌面盒子的外观、显隐方式与系统集成"
  },
  about: {
    eyebrow: "ABOUT DCREEL",
    title: "关于 DCreel",
    summary: "一只藏在 Windows 桌面上的数字鱼篓"
  }
};

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function SplashScreen() {
  return (
    <div className="splash-screen" role="status" aria-label="DCreel 正在启动">
      <div className="splash-spinner" />
    </div>
  );
}

export default function App() {
  const windowLabel = isTauri() ? getCurrentWindow().label : "main";
  return windowLabel === "splash" ? <SplashScreen /> : <DCreelApp />;
}

function DCreelApp() {
  const isNewBoxWindow = isTauri() && getCurrentWindow().label === "new-box";
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [view, setView] = useState<View>("desktop");
  const [modal, setModal] = useState<Modal>(null);
  const [search, setSearch] = useState("");
  const [newKind, setNewKind] = useState<NewFenceKind>("storage");
  const [newTitle, setNewTitle] = useState("");
  const [newColor, setNewColor] = useState("coral");
  const [newContentColor, setNewContentColor] = useState("paper");
  const [newDirectory, setNewDirectory] = useState<string | undefined>();
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [windowMaximized, setWindowMaximized] = useState(false);
  const [appVersion, setAppVersion] = useState("0.1.1");
  const [updater, setUpdater] = useState<UpdaterState>(initialUpdaterState);
  const dashboardRef = useRef<Dashboard | null>(null);
  const availableUpdateRef = useRef<Update | null>(null);
  const autoUpdateStarted = useRef(false);
  const preferenceTimer = useRef<number | undefined>(undefined);
  const folderRefreshTimer = useRef<number | undefined>(undefined);
  const preferenceBufferedRef = useRef<Partial<Preferences>>({});
  const preferenceOptimisticRef = useRef<Partial<Preferences>>({});
  const preferenceSaveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const fenceOptimisticRef = useRef<Map<string, FencePatch>>(new Map());
  const fenceSaveQueuesRef = useRef<Map<string, Promise<void>>>(new Map());

  const refresh = useCallback(async () => {
    try {
      const loaded = await loadDashboard();
      const next = mergeDashboardWithOptimistic(
        loaded,
        preferenceOptimisticRef.current,
        fenceOptimisticRef.current
      );
      dashboardRef.current = next;
      setDashboard(next);
      setLoadError(null);
      return true;
    } catch (error) {
      const message = `读取本地配置失败：${errorMessage(error)}`;
      setToast(message);
      setLoadError(message);
      void writeFrontendLog("error", message, "startup").catch(() => undefined);
      return false;
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    void (async () => {
      try {
        await waitForBackendReady();
      } catch (error) {
        const message = errorMessage(error);
        setLoadError(message);
        void writeFrontendLog("error", message, "startup").catch(() => undefined);
        if (!isNewBoxWindow && isTauri()) {
          await completeStartup().catch(() => undefined);
        }
        return;
      }
      if (disposed) return;
      const loaded = await refresh();
      if (disposed || isNewBoxWindow || !isTauri()) return;
      void writeFrontendLog(
        loaded ? "info" : "error",
        loaded ? "Dashboard initialized" : "Dashboard initialization failed",
        "startup"
      ).catch(() => undefined);
      await completeStartup().catch((error) => {
        void writeFrontendLog(
          "error",
          `Unable to complete native startup: ${errorMessage(error)}`,
          "startup"
        ).catch(() => undefined);
      });
    })();
    return () => {
      disposed = true;
    };
  }, [isNewBoxWindow, refresh]);

  const checkForUpdates = useCallback(async (manual: boolean) => {
    if (!isTauri()) {
      setUpdater({ ...initialUpdaterState, phase: "current" });
      return;
    }
    setUpdater({ ...initialUpdaterState, phase: "checking" });
    void writeFrontendLog("info", `Update check started manual=${manual}`, "updater").catch(
      () => undefined
    );
    try {
      const previous = availableUpdateRef.current;
      availableUpdateRef.current = null;
      if (previous) await previous.close().catch(() => undefined);
      const next = await check({ timeout: 20_000, allowDowngrades: false });
      if (!next) {
        setUpdater({ ...initialUpdaterState, phase: "current" });
        void writeFrontendLog("info", "No newer release is available", "updater").catch(
          () => undefined
        );
        if (manual) setToast("当前已是最新版本");
        return;
      }
      availableUpdateRef.current = next;
      setUpdater({
        phase: "available",
        availableVersion: next.version,
        releaseNotes: next.body?.trim() || undefined,
        downloadedBytes: 0
      });
      void writeFrontendLog(
        "info",
        `Update available current=${next.currentVersion} available=${next.version}`,
        "updater"
      ).catch(() => undefined);
      if (manual) setToast(`发现新版本 ${next.version}`);
    } catch (error) {
      const message = errorMessage(error);
      setUpdater({ ...initialUpdaterState, phase: "error", error: message });
      void writeFrontendLog("error", `Update check failed: ${message}`, "updater").catch(
        () => undefined
      );
      if (manual) setToast(`检查更新失败：${message}`);
    }
  }, []);

  const downloadUpdate = useCallback(async () => {
    const update = availableUpdateRef.current;
    if (!update) {
      setToast("更新信息已失效，请重新检查");
      return;
    }
    let downloadedBytes = 0;
    let totalBytes: number | undefined;
    setUpdater((state) => ({ ...state, phase: "downloading", downloadedBytes: 0 }));
    void writeFrontendLog("info", `Update download started version=${update.version}`, "updater").catch(
      () => undefined
    );
    const onDownloadEvent = (event: DownloadEvent) => {
      if (event.event === "Started") {
        totalBytes = event.data.contentLength;
        setUpdater((state) => ({ ...state, totalBytes }));
      } else if (event.event === "Progress") {
        downloadedBytes += event.data.chunkLength;
        setUpdater((state) => ({ ...state, downloadedBytes, totalBytes }));
      }
    };
    try {
      await update.download(onDownloadEvent, { timeout: 120_000 });
      setUpdater((state) => ({ ...state, phase: "downloaded", downloadedBytes, totalBytes }));
      void writeFrontendLog(
        "info",
        `Update download completed version=${update.version} bytes=${downloadedBytes}`,
        "updater"
      ).catch(() => undefined);
    } catch (error) {
      const message = errorMessage(error);
      setUpdater((state) => ({ ...state, phase: "error", error: message }));
      void writeFrontendLog("error", `Update download failed: ${message}`, "updater").catch(
        () => undefined
      );
      setToast(`下载更新失败：${message}`);
    }
  }, []);

  const installUpdate = useCallback(async () => {
    const update = availableUpdateRef.current;
    if (!update) {
      setToast("更新信息已失效，请重新检查");
      return;
    }
    setUpdater((state) => ({ ...state, phase: "installing" }));
    void writeFrontendLog("info", `Update install started version=${update.version}`, "updater").catch(
      () => undefined
    );
    try {
      await update.install({ restartAfterInstall: true });
    } catch (error) {
      const message = errorMessage(error);
      setUpdater((state) => ({ ...state, phase: "error", error: message }));
      void writeFrontendLog("error", `Update install failed: ${message}`, "updater").catch(
        () => undefined
      );
      setToast(`安装更新失败：${message}`);
    }
  }, []);

  useEffect(() => {
    if (isNewBoxWindow || !isTauri()) return;
    void getVersion()
      .then(setAppVersion)
      .catch((error) =>
        writeFrontendLog(
          "warn",
          `Unable to read application version: ${errorMessage(error)}`,
          "updater"
        ).catch(() => undefined)
      );
  }, [isNewBoxWindow]);

  const dashboardReady = dashboard !== null;
  const desktopVisible = dashboard?.desktopVisible ?? true;

  useEffect(() => {
    if (!dashboardReady || isNewBoxWindow || autoUpdateStarted.current || !isTauri()) return;
    const timer = window.setTimeout(() => {
      if (autoUpdateStarted.current) return;
      autoUpdateStarted.current = true;
      void checkForUpdates(false);
    }, 4_000);
    return () => window.clearTimeout(timer);
  }, [checkForUpdates, dashboardReady, isNewBoxWindow]);

  useEffect(
    () => () => {
      const update = availableUpdateRef.current;
      availableUpdateRef.current = null;
      if (update) void update.close().catch(() => undefined);
    },
    []
  );

  useEffect(() => {
    if (!isTauri() || isNewBoxWindow) return;
    const appWindow = getCurrentWindow();
    let disposed = false;
    let disposeResize: (() => void) | undefined;
    const syncMaximized = async () => {
      try {
        const maximized = await appWindow.isMaximized();
        if (!disposed) setWindowMaximized(maximized);
      } catch {
        // 窗口可能正处于退出流程，不需要为状态同步显示错误提示。
      }
    };
    void syncMaximized();
    void appWindow.onResized(() => void syncMaximized()).then((dispose) => {
      if (disposed) {
        dispose();
      } else {
        disposeResize = dispose;
      }
    });
    return () => {
      disposed = true;
      disposeResize?.();
    };
  }, [isNewBoxWindow]);

  const toggleWindowMaximized = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const appWindow = getCurrentWindow();
      await appWindow.toggleMaximize();
      setWindowMaximized(await appWindow.isMaximized());
    } catch (error) {
      setToast(`无法切换窗口大小：${errorMessage(error)}`);
    }
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let disposeState: (() => void) | undefined;
    let disposeNavigate: (() => void) | undefined;
    let disposeFolder: (() => void) | undefined;
    let disposeNotification: (() => void) | undefined;
    if (!isNewBoxWindow) {
      void listen("creel://state-changed", () => void refresh()).then((dispose) => {
        disposeState = dispose;
      });
    }
    const navigate = (payload: string | null) => {
      if (payload === "new-storage-box") {
        setView("desktop");
        setNewKind("storage");
        setNewTitle("");
        setNewDirectory(undefined);
        setNewColor("coral");
        setNewContentColor("paper");
        setModal("new");
      } else if (payload === "new-mapped-box") {
        setView("desktop");
        setNewKind("mapped");
        setNewTitle("新的映射盒子");
        setNewDirectory(undefined);
        setNewColor("sage");
        setNewContentColor("paper");
        setModal("new");
      }
    };
    void listen<string>("creel://navigate", ({ payload }) => {
      navigate(payload);
      if (payload === "new-storage-box" || payload === "new-mapped-box") {
        void takePendingNavigation();
      }
    }).then(async (dispose) => {
      disposeNavigate = dispose;
      if (isNewBoxWindow) {
        navigate(await takePendingNavigation());
      }
    });
    if (!isNewBoxWindow) {
      void listen("creel://folder-changed", () => {
        window.clearTimeout(folderRefreshTimer.current);
        folderRefreshTimer.current = window.setTimeout(() => void refresh(), 180);
      }).then((dispose) => {
        disposeFolder = dispose;
      });
      void listen<string>("creel://notification", ({ payload }) => setToast(payload)).then(
        (dispose) => {
          disposeNotification = dispose;
        }
      );
    }
    return () => {
      disposeState?.();
      disposeNavigate?.();
      disposeFolder?.();
      disposeNotification?.();
      window.clearTimeout(folderRefreshTimer.current);
    };
  }, [isNewBoxWindow, refresh]);

  useEffect(() => {
    dashboardRef.current = dashboard;
  }, [dashboard]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3200);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const mutateFence = async (id: string, patch: FencePatch) => {
    const dashboard = dashboardRef.current;
    if (!dashboard) return;
    const current = dashboard.fences.find((fence) => fence.id === id);
    if (!current) return;
    const updated = { ...current, ...patch };
    const next = {
      ...dashboard,
      fences: dashboard.fences.map((fence) => (fence.id === id ? updated : fence))
    };
    dashboardRef.current = next;
    setDashboard(next);
    fenceOptimisticRef.current.set(id, {
      ...(fenceOptimisticRef.current.get(id) ?? {}),
      ...patch
    });

    const previousSave = fenceSaveQueuesRef.current.get(id) ?? Promise.resolve();
    const save = previousSave
      .catch(() => undefined)
      .then(async () => {
        await saveFence(id, patch);
        const optimistic = fenceOptimisticRef.current.get(id);
        if (optimistic) {
          const remaining = clearMatchingPatch(optimistic, patch);
          if (Object.keys(remaining).length) {
            fenceOptimisticRef.current.set(id, remaining);
          } else {
            fenceOptimisticRef.current.delete(id);
          }
        }
      });
    fenceSaveQueuesRef.current.set(id, save);
    try {
      await save;
    } catch (error) {
      const optimistic = fenceOptimisticRef.current.get(id);
      if (optimistic) {
        const remaining = clearMatchingPatch(optimistic, patch);
        if (Object.keys(remaining).length) {
          fenceOptimisticRef.current.set(id, remaining);
        } else {
          fenceOptimisticRef.current.delete(id);
        }
      }
      setToast(errorMessage(error));
      void refresh();
    } finally {
      if (fenceSaveQueuesRef.current.get(id) === save) {
        fenceSaveQueuesRef.current.delete(id);
      }
    }
  };

  const renameFence = (fence: Fence) => {
    const title = window.prompt("新的盒子名称", fence.title)?.trim();
    if (title && title !== fence.title) void mutateFence(fence.id, { title });
  };

  const showActualDesktop = async () => {
    try {
      if (!desktopVisible) {
        const visible = await setDesktopVisibility(true);
        setDashboard((state) => {
          if (!state) return state;
          const updated = { ...state, desktopVisible: visible };
          dashboardRef.current = updated;
          return updated;
        });
      }
      await getCurrentWindow().minimize();
    } catch (error) {
      setToast(errorMessage(error));
    }
  };

  const chooseFolder = async () => {
    if (!isTauri()) {
      setNewDirectory("C:\\Users\\DCreel\\Documents");
      return;
    }
    const selected = await open({
      directory: true,
      multiple: false,
      title: newKind === "storage" ? "选择收纳盒文件的存放位置" : "选择要映射到桌面的文件夹"
    });
    if (selected) {
      setNewDirectory(selected);
      if (newKind === "mapped" && newTitle === "新的映射盒子") {
        setNewTitle(basename(selected));
      }
    }
  };

  const showNewFence = (kind: NewFenceKind) => {
    setNewKind(kind);
    setNewTitle(kind === "storage" ? "" : "新的映射盒子");
    setNewDirectory(undefined);
    setNewColor(kind === "storage" ? "coral" : "sage");
    setNewContentColor("paper");
    setModal("new");
  };

  const dismissNewFence = () => {
    setModal(null);
    if (isNewBoxWindow) {
      void getCurrentWindow().hide();
    }
  };

  const submitNewFence = async () => {
    if (!newTitle.trim()) return;
    if (!newDirectory) {
      setToast(newKind === "storage" ? "请先选择文件存放位置" : "请先选择要映射的文件夹");
      return;
    }
    setBusy(true);
    try {
      const input = {
        title: newTitle.trim(),
        color: newColor,
        contentColor: newContentColor,
        directory: newDirectory
      };
      const fence =
        newKind === "storage" ? await createStorageBox(input) : await createMappedBox(input);
      setDashboard((state) => {
        if (!state) return state;
        const updated = { ...state, fences: [...state.fences, fence] };
        dashboardRef.current = updated;
        return updated;
      });
      dismissNewFence();
      setToast(newKind === "storage" ? "收纳盒已创建并映射到桌面" : "映射盒子已创建");
    } catch (error) {
      setToast(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (fence: Fence) => {
    if (!window.confirm(`移除「${fence.title}」？磁盘里的文件不会被删除。`)) return;
    try {
      await fenceSaveQueuesRef.current.get(fence.id)?.catch(() => undefined);
      await deleteFence(fence.id);
      fenceOptimisticRef.current.delete(fence.id);
      setDashboard((state) => {
        if (!state) return state;
        const updated = {
          ...state,
          fences: state.fences.filter(({ id }) => id !== fence.id)
        };
        dashboardRef.current = updated;
        return updated;
      });
      setToast("盒子显示已移除；真实文件夹和其中内容仍然保留");
    } catch (error) {
      setToast(errorMessage(error));
    }
  };

  const enqueuePreferenceSave = (patch: Partial<Preferences>) => {
    const save = preferenceSaveQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        const saved = await savePreferences(patch);
        const optimistic = preferenceOptimisticRef.current;
        const remaining = clearMatchingPatch(optimistic, patch);
        preferenceOptimisticRef.current = remaining;
        setDashboard((state) => {
          if (!state) return state;
          const updated = {
            ...state,
            preferences: { ...saved, ...remaining }
          };
          dashboardRef.current = updated;
          return updated;
        });
      });
    preferenceSaveQueueRef.current = save.catch((error) => {
      const optimistic = preferenceOptimisticRef.current;
      const remaining = clearMatchingPatch(optimistic, patch);
      preferenceOptimisticRef.current = remaining;
      setToast(`设置没有保存：${errorMessage(error)}`);
      void refresh();
    });
  };

  const flushPreferenceChanges = () => {
    window.clearTimeout(preferenceTimer.current);
    preferenceTimer.current = undefined;
    const patch = preferenceBufferedRef.current;
    preferenceBufferedRef.current = {};
    if (Object.keys(patch).length) enqueuePreferenceSave(patch);
  };

  const updatePreference = <K extends keyof Preferences>(
    key: K,
    value: Preferences[K]
  ) => {
    const current = dashboardRef.current;
    if (!current) return;
    const preferences = { ...current.preferences, [key]: value };
    const next = { ...current, preferences };
    dashboardRef.current = next;
    setDashboard(next);
    preferenceBufferedRef.current = { ...preferenceBufferedRef.current, [key]: value };
    preferenceOptimisticRef.current = { ...preferenceOptimisticRef.current, [key]: value };
    window.clearTimeout(preferenceTimer.current);
    preferenceTimer.current = window.setTimeout(flushPreferenceChanges, 220);
  };

  useEffect(
    () => () => {
      flushPreferenceChanges();
    },
    []
  );

  const filteredCount = useMemo(() => {
    if (!dashboard) return 0;
    return dashboard.fences.reduce((sum, fence) => sum + fence.itemCount, 0);
  }, [dashboard]);

  if (!dashboard) {
    if (isNewBoxWindow) return null;
    return (
      <div className={`boot-screen ${loadError ? "failed" : ""}`}>
        {loadError ? (
          <div className="startup-error">
            <b>DCreel 无法读取本地配置</b>
            <span>{loadError}</span>
            <button className="button secondary" onClick={() => void openLogDirectory()}>
              <FolderOpen /> 打开日志目录
            </button>
          </div>
        ) : (
          <div className="boot-spinner" />
        )}
      </div>
    );
  }

  return (
    <div className={isNewBoxWindow ? "new-box-window" : "app-shell"}>
      {!isNewBoxWindow && (
        <>
      <div className="titlebar" data-tauri-drag-region>
        <div className="brand-mini" data-tauri-drag-region>
          <img src="/creel-icon.png" alt="" />
          <span data-tauri-drag-region>DCreel</span>
        </div>
        <div className="titlebar-status" data-tauri-drag-region>
          <ShieldCheck size={13} /> 本地运行 · 数据不离开电脑
        </div>
        <div className="window-actions">
          <button aria-label="最小化" onClick={() => void getCurrentWindow().minimize()}>
            <Minus />
          </button>
          <button
            aria-label={windowMaximized ? "还原窗口" : "最大化窗口"}
            title={windowMaximized ? "还原窗口" : "最大化窗口"}
            onClick={() => void toggleWindowMaximized()}
          >
            {windowMaximized ? <Copy /> : <Square />}
          </button>
          <button aria-label="关闭到托盘" onClick={() => void getCurrentWindow().close()}>
            <X />
          </button>
        </div>
      </div>

      <div className="workspace">
        <aside className="sidebar">
          <div className="logo-card">
            <img src="/creel-icon.png" alt="DCreel" />
          </div>
          <nav aria-label="主要功能">
            <button
              className={view === "desktop" ? "active" : ""}
              onClick={() => setView("desktop")}
              title="桌面盒子"
            >
              <LayoutGrid />
              <span>盒子</span>
            </button>
            <button
              className={view === "settings" ? "active" : ""}
              onClick={() => setView("settings")}
              title="设置"
            >
              <Settings />
              <span>设置</span>
            </button>
          </nav>
          <button
            className={`help-button ${view === "about" ? "active" : ""}`}
            onClick={() => setView("about")}
            title="关于 DCreel"
          >
            <CircleHelp />
            <span>关于</span>
          </button>
        </aside>

        <main className="main-area">
          <header className="page-header">
            <div>
              <p className="eyebrow">
                {viewMeta[view].eyebrow}
              </p>
              <h1>{viewMeta[view].title}</h1>
              <span className="page-summary">
                {view === "desktop"
                  ? `${dashboard.fences.length} 个盒子 · ${filteredCount} 个项目 · ${viewMeta.desktop.summary}`
                  : viewMeta[view].summary}
              </span>
            </div>
            {view === "desktop" && (
              <div className="header-actions">
                <label className="search-box">
                  <Search />
                  <input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder="搜索盒子"
                  />
                  {search && (
                    <button onClick={() => setSearch("")} aria-label="清除搜索">
                      <X />
                    </button>
                  )}
                </label>
                <button className="refresh-button" onClick={() => void refresh()} title="刷新文件列表">
                  <RefreshCw />
                </button>
                <button
                  className={`refresh-button ${desktopVisible ? "desktop-visible" : ""}`}
                  onClick={() => {
                    const visible = !desktopVisible;
                    void setDesktopVisibility(visible)
                      .then((actualVisible) => {
                        setDashboard((state) => {
                          if (!state) return state;
                          const updated = { ...state, desktopVisible: actualVisible };
                          dashboardRef.current = updated;
                          return updated;
                        });
                        setToast(actualVisible ? "桌面盒子已显示" : "桌面盒子已暂时隐藏");
                      })
                      .catch((error) => setToast(errorMessage(error)));
                  }}
                  title={desktopVisible ? "暂时隐藏桌面盒子" : "显示桌面盒子"}
                >
                  {desktopVisible ? <Eye /> : <EyeOff />}
                </button>
                <div className="add-menu">
                  <button className="button primary" onClick={() => showNewFence("storage")}>
                    <Plus /> 新建盒子 <ChevronDown />
                  </button>
                  <div className="add-menu-popover">
                    <button onClick={() => showNewFence("storage")}>
                      <Inbox />
                      <span><b>新建收纳盒</b><small>自选存放位置，创建文件夹并映射到桌面</small></span>
                    </button>
                    <button onClick={() => showNewFence("mapped")}>
                      <Link2 />
                      <span><b>映射盒子</b><small>选择任意现有文件夹，在桌面显示为盒子</small></span>
                    </button>
                  </div>
                </div>
              </div>
            )}
          </header>

          {view === "desktop" && (
            <FenceManagerView
              fences={dashboard.fences}
              search={search}
              desktopVisible={desktopVisible}
              onShowDesktop={() => void showActualDesktop()}
              onOpen={(path) => void openItem(path).catch((error) => setToast(errorMessage(error)))}
              onRename={renameFence}
              onPatch={(fence, patch) => void mutateFence(fence.id, patch)}
              onDelete={(fence) => void remove(fence)}
              onNew={() => showNewFence("storage")}
            />
          )}

          {view === "settings" && (
            <SettingsView
              preferences={dashboard.preferences}
              onChange={updatePreference}
              onError={(error) => setToast(errorMessage(error))}
              onQuit={() => {
                flushPreferenceChanges();
                if (window.confirm("确定要彻底退出 DCreel 吗？文件夹和文件不会删除。")) {
                  const pendingSaves = [
                    preferenceSaveQueueRef.current,
                    ...fenceSaveQueuesRef.current.values()
                  ];
                  void Promise.allSettled(pendingSaves)
                    .then(() => quitApplication())
                    .catch((error) => setToast(`无法退出 DCreel：${errorMessage(error)}`));
                }
              }}
            />
          )}

          {view === "about" && (
            <AboutView
              currentVersion={appVersion}
              updater={updater}
              onCheck={() => void checkForUpdates(true)}
              onDownload={() => void downloadUpdate()}
              onInstall={() => void installUpdate()}
              onOpenLogs={() => void openLogDirectory().catch((error) => setToast(errorMessage(error)))}
              onError={(error) => setToast(errorMessage(error))}
            />
          )}
        </main>
      </div>
        </>
      )}

      {modal === "new" && (
        <div className="modal-backdrop" onMouseDown={dismissNewFence}>
          <div className="modal-card" onMouseDown={(event) => event.stopPropagation()}>
            <button className="modal-close" onClick={dismissNewFence}><X /></button>
            <div className="modal-icon" style={{ background: resolvedColor(newColor) }}>
              {newKind === "storage" ? <Inbox /> : <FolderInput />}
            </div>
            <p className="eyebrow">{newKind === "storage" ? "NEW STORAGE BOX" : "MAPPED BOX"}</p>
            <h2>{newKind === "storage" ? "新建收纳盒" : "新建映射盒子"}</h2>
            <p className="modal-copy">
              {newKind === "storage"
                ? "先选择文件存放位置，再输入盒子名称。DCreel 会在所选位置创建同名文件夹，并把它映射成桌面盒子。"
                : "选择任意位置的现有文件夹，把它映射到桌面并显示成盒子；不会移动、复制或隐藏原文件夹，盒子中的操作直接作用于原内容。"}
            </p>
            <label className="field-label">
              {newKind === "storage" ? "文件存放位置" : "要映射的文件夹"}
              <button className="folder-picker" onClick={() => void chooseFolder()} autoFocus>
                <FolderOpen />
                <span>{newDirectory || (newKind === "storage" ? "选择一个存放位置…" : "选择一个现有文件夹…")}</span>
                <ChevronRight />
              </button>
            </label>
            <label className="field-label">
              盒子名称
              <input
                value={newTitle}
                onChange={(event) => setNewTitle(event.target.value)}
                placeholder={newKind === "storage" ? "输入新文件夹和盒子的名称" : "输入盒子显示名称"}
              />
            </label>
            <div className="field-label">
              标题颜色
              <AppearanceColorPicker value={newColor} onChange={setNewColor} />
            </div>
            <div className="field-label">
              图标区域
              <AppearanceColorPicker
                value={newContentColor}
                onChange={setNewContentColor}
                contentMaterial
              />
            </div>
            <div className="modal-actions">
              <button className="button ghost" onClick={dismissNewFence}>取消</button>
              <button
                className="button primary"
                onClick={() => void submitNewFence()}
                disabled={busy || !newDirectory || !newTitle.trim()}
              >
                {busy ? <RefreshCw className="spin" /> : <Plus />}
                {newKind === "storage" ? "创建收纳盒" : "创建映射盒子"}
              </button>
            </div>
          </div>
        </div>
      )}

      {toast && <div className="toast"><Check /> {toast}</div>}
    </div>
  );
}

interface FenceManagerProps {
  fences: Fence[];
  search: string;
  desktopVisible: boolean;
  onShowDesktop: () => void;
  onOpen: (path: string) => void;
  onRename: (fence: Fence) => void;
  onPatch: (fence: Fence, patch: FencePatch) => void;
  onDelete: (fence: Fence) => void;
  onNew: () => void;
}

function FenceManagerView({
  fences,
  search,
  desktopVisible,
  onShowDesktop,
  onOpen,
  onRename,
  onPatch,
  onDelete,
  onNew
}: FenceManagerProps) {
  const query = search.trim().toLocaleLowerCase();
  const visibleFences = query
    ? fences.filter(
        (fence) =>
          fence.title.toLocaleLowerCase().includes(query) ||
          fence.directory.toLocaleLowerCase().includes(query)
      )
    : fences;

  return (
    <section className="content-page fence-manager-page">
      <div className="desktop-source-card">
        <span className="desktop-source-icon"><MonitorUp /></span>
        <div>
          <b>所有桌面盒子都是本地文件夹映射</b>
          <p>移动、缩放、选择和文件操作都在桌面完成；原文件夹保留在你选择的磁盘位置。</p>
        </div>
        <button className="button primary" onClick={onShowDesktop}>
          {desktopVisible ? <MonitorUp /> : <Eye />} {desktopVisible ? "最小化 DCreel，回到桌面" : "显示桌面盒子"}
        </button>
      </div>

      <div className="manager-heading">
        <div><h2>全部盒子</h2><span>{visibleFences.length} / {fences.length}</span></div>
        <button className="button secondary" onClick={onNew}><Plus /> 新建收纳盒</button>
      </div>

      {visibleFences.length > 0 ? (
        <div className="fence-manager-grid">
          {visibleFences.map((fence) => (
            <article
              key={fence.id}
              className="fence-manager-card"
              style={{ "--accent": resolvedColor(fence.color) } as CSSProperties}
            >
              <div className="manager-card-icon">
                <Link2 />
              </div>
              <div className="manager-card-copy">
                <div className="manager-card-title">
                  <h3>{fence.title}</h3>
                  {fence.locked && <Lock />}
                </div>
                <p>文件夹映射 · {fence.itemCount} 个项目</p>
                <span title={fence.directory}>{fence.directory}</span>
              </div>
              <div className="manager-card-actions">
                <button onClick={() => onOpen(fence.directory)} title="打开文件夹"><FolderOpen /></button>
                <button onClick={() => onRename(fence)} title="修改名称"><Pencil /></button>
                <button
                  onClick={() => onPatch(fence, { locked: !fence.locked })}
                  title={fence.locked ? "解除位置锁定" : "锁定位置"}
                >
                  {fence.locked ? <Unlock /> : <Lock />}
                </button>
                <button className="danger" onClick={() => onDelete(fence)} title="移除盒子显示（不删除文件夹）"><Trash2 /></button>
              </div>
              <div className="manager-appearance-controls">
                <div className="manager-color-row">
                  <span>标题</span>
                  <AppearanceColorPicker
                    value={fence.color}
                    onChange={(color) => onPatch(fence, { color })}
                    compact
                  />
                </div>
                <div className="manager-color-row">
                  <span>图标区</span>
                  <AppearanceColorPicker
                    value={fence.contentColor}
                    onChange={(contentColor) => onPatch(fence, { contentColor })}
                    contentMaterial
                    compact
                  />
                </div>
              </div>
            </article>
          ))}
        </div>
      ) : (
        <div className="manager-empty">
          <Boxes />
          <h2>{fences.length ? "没有匹配的盒子" : "桌面上还没有盒子"}</h2>
          <p>{fences.length ? "换一个名称或文件夹路径搜索" : "新建收纳盒或映射现有文件夹后，盒子会出现在桌面上。"}</p>
          {!fences.length && <button className="button primary" onClick={onNew}><Plus /> 新建第一个桌面盒子</button>}
        </div>
      )}
    </section>
  );
}

interface AppearanceColorPickerProps {
  value: string;
  onChange: (value: string) => void;
  contentMaterial?: boolean;
  compact?: boolean;
}

function AppearanceColorPicker({
  value,
  onChange,
  contentMaterial = false,
  compact = false
}: AppearanceColorPickerProps) {
  const customSelected = /^#[0-9a-f]{6}$/i.test(value);
  return (
    <div className={`appearance-color-picker ${compact ? "compact" : ""}`}>
      {contentMaterial && (
        <button
          type="button"
          className={`appearance-swatch paper-swatch ${value === "paper" ? "selected" : ""}`}
          onClick={() => onChange("paper")}
          title="纸色"
          aria-label="使用纸色背景"
        >
          {value === "paper" && <Check />}
        </button>
      )}
      {colors.map((color) => (
        <button
          type="button"
          key={color}
          className={`appearance-swatch ${value === color ? "selected" : ""}`}
          style={{ background: resolvedColor(color) }}
          onClick={() => onChange(color)}
          title={`使用 ${color} 颜色`}
          aria-label={`使用 ${color} 颜色`}
        >
          {value === color && <Check />}
        </button>
      ))}
      <label
        className={`appearance-swatch custom-color-swatch ${customSelected ? "selected" : ""}`}
        style={{ "--custom-color": customColorValue(value, contentMaterial ? "paper" : "coral") } as CSSProperties}
        title="从调色板选择任意颜色"
      >
        <Palette />
        <input
          type="color"
          value={customColorValue(value, contentMaterial ? "paper" : "coral")}
          onChange={(event) => onChange(event.target.value.toUpperCase())}
          aria-label="从调色板选择任意颜色"
        />
      </label>
      {contentMaterial && (
        <button
          type="button"
          className={`appearance-swatch frosted-swatch ${value === "frosted" ? "selected" : ""}`}
          onClick={() => onChange("frosted")}
          title="透明磨砂"
          aria-label="使用透明磨砂背景"
        >
          <Sparkles />
        </button>
      )}
    </div>
  );
}

interface SettingsViewProps {
  preferences: Preferences;
  onChange: <K extends keyof Preferences>(key: K, value: Preferences[K]) => void;
  onError: (error: unknown) => void;
  onQuit: () => void;
}

function SettingsView({ preferences, onChange, onError, onQuit }: SettingsViewProps) {
  return (
    <section className="content-page settings-page">
      <div className="settings-column">
        <div className="settings-section-title"><AppWindow /><div><h2>外观与交互</h2><p>调整桌面盒子的显示与操作方式</p></div></div>
        <div className="settings-card">
          <div className="setting-row range-row">
            <div><b>标题区域不透明度</b><span>单独控制标题、项目计数和窄移动条</span></div>
            <EditableRangeControl
              ariaLabel="标题区域不透明度"
              min={0}
              max={100}
              value={Math.round(preferences.titleOpacity * 100)}
              suffix="%"
              onChange={(value) => onChange("titleOpacity", value / 100)}
            />
          </div>
          <div className="setting-row range-row">
            <div><b>图标区域不透明度</b><span>只改变图标下方的颜色或磨砂遮罩，文件内容保持清晰</span></div>
            <EditableRangeControl
              ariaLabel="图标区域不透明度"
              min={0}
              max={100}
              value={Math.round(preferences.contentOpacity * 100)}
              suffix="%"
              onChange={(value) => onChange("contentOpacity", value / 100)}
            />
          </div>
          <SettingSwitch title="显示盒子外边线" detail="保留一圈与标题颜色一致的盒子边界" checked={preferences.showFenceBorder} onChange={(value) => onChange("showFenceBorder", value)} />
          {preferences.showFenceBorder && (
            <div className="setting-row range-row">
              <div><b>外边线不透明度</b><span>边线独立于标题和图标区域透明度</span></div>
              <EditableRangeControl
                ariaLabel="外边线不透明度"
                min={0}
                max={100}
                value={Math.round(preferences.fenceBorderOpacity * 100)}
                suffix="%"
                onChange={(value) => onChange("fenceBorderOpacity", value / 100)}
              />
            </div>
          )}
          <div className="setting-row range-row">
            <div><b>文件图标大小</b><span>文件名、图标间距和网格密度会按比例一起缩放</span></div>
            <EditableRangeControl
              ariaLabel="文件图标大小"
              min={36}
              max={64}
              value={preferences.iconSize}
              suffix="px"
              onChange={(value) => onChange("iconSize", value)}
            />
          </div>
          <div className="setting-row range-row">
            <div><b>新盒子默认宽度</b><span>新建收纳盒和映射盒子会使用这个宽度</span></div>
            <EditableRangeControl
              ariaLabel="新盒子默认宽度"
              min={244}
              max={800}
              value={preferences.defaultFenceWidth}
              suffix="px"
              onChange={(value) => onChange("defaultFenceWidth", value)}
            />
          </div>
          <div className="setting-row range-row">
            <div><b>新盒子默认高度</b><span>可在盒子右键菜单中随时恢复为这个高度</span></div>
            <EditableRangeControl
              ariaLabel="新盒子默认高度"
              min={148}
              max={700}
              value={preferences.defaultFenceHeight}
              suffix="px"
              onChange={(value) => onChange("defaultFenceHeight", value)}
            />
          </div>
          <SettingSwitch title="幽灵模式" detail={preferences.ghostModeTrigger === "hotkey" ? "使用全局快捷键完全隐藏或恢复所有盒子" : "鼠标离开后把整个盒子及其内容淡化到设定透明度"} checked={preferences.ghostMode} onChange={(value) => onChange("ghostMode", value)} />
          {preferences.ghostMode && (
            <div className="setting-row ghost-trigger-row">
              <div><b>消失方式</b><span>自动淡化，或者通过组合快捷键完全显隐</span></div>
              <div className="setting-segments">
                <button type="button" className={preferences.ghostModeTrigger === "automatic" ? "active" : ""} onClick={() => onChange("ghostModeTrigger", "automatic")}>自动淡化</button>
                <button type="button" className={preferences.ghostModeTrigger === "hotkey" ? "active" : ""} onClick={() => onChange("ghostModeTrigger", "hotkey")}>快捷键显隐</button>
              </div>
            </div>
          )}
          {preferences.ghostMode && preferences.ghostModeTrigger === "automatic" && (
            <div className="setting-row range-row">
              <div><b>淡化后透明度</b><span>鼠标离开盒子后的整体透明度；0% 为完全透明</span></div>
              <EditableRangeControl
                ariaLabel="淡化后透明度"
                min={0}
                max={100}
                value={Math.round(preferences.ghostOpacity * 100)}
                suffix="%"
                onChange={(value) => onChange("ghostOpacity", value / 100)}
              />
            </div>
          )}
          {preferences.ghostMode && preferences.ghostModeTrigger === "hotkey" && (
            <label className="setting-row hotkey-row">
              <div><b>显隐快捷键</b><span>按住全部按键后松开保存；Z+X 等普通键组合不会拦截输入，触发时当前软件也会收到这些按键</span></div>
              <HotkeyInput
                value={preferences.ghostHotkey}
                onChange={(value) => onChange("ghostHotkey", value)}
                onCaptureChange={(active) => {
                  void setHotkeyCaptureActive(active).catch(onError);
                }}
              />
            </label>
          )}
          <SettingSwitch title="隐藏盒子标题" detail="开启后仅保留 16px 窄移动条，不显示标题和项目计数" checked={!preferences.showFenceTitles} onChange={(value) => onChange("showFenceTitles", !value)} />
          <SettingSwitch title="显示隐藏文件" detail="所有盒子的文件列表都包括点开头或系统隐藏的项目" checked={preferences.showHiddenFiles} onChange={(value) => onChange("showHiddenFiles", value)} />
        </div>
        <div className="settings-section-title"><Settings /><div><h2>系统</h2><p>启动与桌面集成</p></div></div>
        <div className="settings-card">
          <SettingSwitch title="开机自动启动" detail="登录 Windows 后在托盘中运行 DCreel" checked={preferences.startOnBoot} onChange={(value) => onChange("startOnBoot", value)} />
          <SettingSwitch title="隐藏托盘图标" detail="隐藏 Windows 通知区域中的 DCreel 图标；仍可通过桌面右键菜单打开或彻底退出" checked={!preferences.showTrayIcon} onChange={(value) => onChange("showTrayIcon", !value)} />
          <SettingSwitch title="桌面常驻盒子" detail="把已映射的文件夹以可交互盒子显示在桌面层；关闭后隐藏所有盒子，原文件夹不受影响" checked={preferences.desktopMode} onChange={(value) => onChange("desktopMode", value)} />
          <SettingSwitch title="桌面右键菜单" detail="在桌面经典右键菜单中加入打开、新建、显隐和彻底退出 DCreel" checked={preferences.desktopContextMenu} onChange={(value) => onChange("desktopContextMenu", value)} />
          <div className="setting-row">
            <div><b>彻底退出 DCreel</b><span>关闭主程序、桌面盒子和后台 Host；不会删除任何文件</span></div>
            <button type="button" className="button secondary" onClick={onQuit}><Power /> 退出 DCreel</button>
          </div>
        </div>
      </div>
    </section>
  );
}

interface AboutViewProps {
  currentVersion: string;
  updater: UpdaterState;
  onCheck: () => void;
  onDownload: () => void;
  onInstall: () => void;
  onOpenLogs: () => void;
  onError: (error: unknown) => void;
}

function AboutView({
  currentVersion,
  updater,
  onCheck,
  onDownload,
  onInstall,
  onOpenLogs,
  onError
}: AboutViewProps) {
  const progress = updater.totalBytes
    ? Math.min(100, Math.round((updater.downloadedBytes / updater.totalBytes) * 100))
    : undefined;
  const status = (() => {
    switch (updater.phase) {
      case "checking":
        return "正在检查 GitHub Releases";
      case "current":
        return "当前已是最新版本";
      case "available":
        return `发现新版本 ${updater.availableVersion}`;
      case "downloading":
        return updater.totalBytes
          ? `正在下载 ${formatBytes(updater.downloadedBytes)} / ${formatBytes(updater.totalBytes)}`
          : `正在下载 ${formatBytes(updater.downloadedBytes)}`;
      case "downloaded":
        return `版本 ${updater.availableVersion} 已下载，可以安装`;
      case "installing":
        return "正在启动更新安装程序";
      case "error":
        return `更新检查或安装失败：${updater.error ?? "未知错误"}`;
      default:
        return "从 GitHub Releases 检查已签名的新版本";
    }
  })();

  return (
    <section className="content-page about-page">
      <div className="about-hero">
        <img src="/creel-icon.png" alt="DCreel 应用图标" />
        <div>
          <span className="hero-kicker"><Inbox /> DESKTOP DCREEL</span>
          <h2>DCreel -- 桌面鱼篓</h2>
          <p>把不开心的藏起来，把摸鱼的也藏起来。</p>
          <span className="about-version">Version {currentVersion} · Tauri 2 · Rust Native</span>
          <a
            className="about-repository"
            href={PROJECT_REPOSITORY_URL}
            target="_blank"
            rel="noreferrer"
            onClick={(event) => {
              event.preventDefault();
              void openProjectRepository().catch(onError);
            }}
          >
            <GitFork />
            <span>{PROJECT_REPOSITORY_URL}</span>
            <ExternalLink />
          </a>
        </div>
      </div>

      <section className={`updater-panel ${updater.phase === "error" ? "has-error" : ""}`}>
        <div className="updater-icon">
          {updater.phase === "current" ? <CircleCheck /> : <Download />}
        </div>
        <div className="updater-copy">
          <div className="updater-heading">
            <b>软件更新</b>
            <span>当前版本 {currentVersion}</span>
          </div>
          <p>{status}</p>
          {updater.phase === "downloading" && (
            <div className="update-progress" aria-label="更新下载进度">
              <span style={{ width: progress === undefined ? "18%" : `${progress}%` }} />
            </div>
          )}
          {updater.releaseNotes && (
            <details className="release-notes">
              <summary>查看版本说明</summary>
              <p>{updater.releaseNotes}</p>
            </details>
          )}
        </div>
        <div className="updater-actions">
          {updater.phase === "available" && (
            <button className="button primary" onClick={onDownload}>
              <Download /> 下载更新
            </button>
          )}
          {updater.phase === "downloaded" && (
            <button className="button primary" onClick={onInstall}>
              <RefreshCw /> 安装并重启
            </button>
          )}
          {(updater.phase === "checking" ||
            updater.phase === "downloading" ||
            updater.phase === "installing") && (
            <button className="button secondary" disabled>
              <RefreshCw className="spin" />
              {updater.phase === "checking"
                ? "检查中"
                : updater.phase === "downloading"
                  ? "下载中"
                  : "安装中"}
            </button>
          )}
          {(updater.phase === "idle" ||
            updater.phase === "current" ||
            updater.phase === "error") && (
            <button className="button secondary" onClick={onCheck}>
              <RefreshCw /> 检查更新
            </button>
          )}
          <button className="button ghost updater-log-button" onClick={onOpenLogs}>
            <FolderOpen /> 日志
          </button>
        </div>
      </section>

      <article className="origin-card">
        <p className="eyebrow">DCreel</p>
        <h2>装鱼的篓子</h2>
        <p><i>“鱼”是摸鱼的“鱼”</i></p>
        <p>没做完的需求文档、永远改不完的表格、不敢点开的会议纪要，暂时不想面对的“不开心”，都藏起来。</p>
        <p>游戏图标、视频站快捷方式、闲聊窗口，以及摸鱼时留下的证据，不太方便见光的“快乐”，都藏起来。</p>
      </article>

      <div className="about-facts">
        <article><FolderOpen /><div><b>文件夹映射</b><span>桌面盒子直接对应你选择的本地目录，资源管理器和上传文件窗口都能正常找到。</span></div></article>
        <article><EyeOff /><div><b>想藏就藏</b><span>自动淡化或全局快捷键显隐，让快乐和不开心按需要退场。</span></div></article>
        <article><MonitorUp /><div><b>留在桌面层</b><span>盒子属于桌面，不会覆盖正在使用的普通应用窗口。</span></div></article>
        <article><ShieldCheck /><div><b>完全本地</b><span>布局与文件操作都在本机完成，不依赖云端服务。</span></div></article>
      </div>
    </section>
  );
}

interface EditableRangeControlProps {
  ariaLabel: string;
  min: number;
  max: number;
  value: number;
  suffix: string;
  onChange: (value: number) => void;
}

function canonicalRangeValue(value: number, min: number, max: number): number {
  return Math.round(Math.min(max, Math.max(min, value)));
}

function EditableRangeControl({
  ariaLabel,
  min,
  max,
  value,
  suffix,
  onChange
}: EditableRangeControlProps) {
  const canonicalValue = canonicalRangeValue(value, min, max);
  const [draft, setDraft] = useState(String(canonicalValue));
  const editing = useRef(false);

  useEffect(() => {
    if (!editing.current) setDraft(String(canonicalValue));
  }, [canonicalValue]);

  const parsedDraft = Number(draft);
  const validDraft =
    draft.trim() !== "" &&
    Number.isFinite(parsedDraft) &&
    Number.isInteger(parsedDraft) &&
    parsedDraft >= min &&
    parsedDraft <= max;

  const commitDraft = () => {
    editing.current = false;
    const next = Number.isFinite(parsedDraft)
      ? canonicalRangeValue(parsedDraft, min, max)
      : canonicalValue;
    setDraft(String(next));
    if (next !== canonicalValue) onChange(next);
  };

  return (
    <div className="range-control">
      <input
        className="range-slider"
        type="range"
        min={min}
        max={max}
        step={1}
        value={canonicalValue}
        aria-label={`${ariaLabel}滑块`}
        onChange={(event) => {
          const next = Number(event.target.value);
          setDraft(String(next));
          onChange(next);
        }}
      />
      <label className="range-number" title={`可输入 ${min} 到 ${max}`}>
        <input
          type="number"
          min={min}
          max={max}
          step={1}
          value={draft}
          inputMode="numeric"
          aria-label={`${ariaLabel}数值`}
          aria-invalid={editing.current && !validDraft}
          onFocus={(event) => {
            editing.current = true;
            event.currentTarget.select();
          }}
          onChange={(event) => {
            const nextDraft = event.target.value;
            setDraft(nextDraft);
            const next = Number(nextDraft);
            if (
              nextDraft.trim() !== "" &&
              Number.isFinite(next) &&
              Number.isInteger(next) &&
              next >= min &&
              next <= max
            ) {
              onChange(next);
            }
          }}
          onBlur={commitDraft}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
          }}
        />
        <span>{suffix}</span>
      </label>
    </div>
  );
}

const hotkeyModifierOrder = ["Ctrl", "Alt", "Shift", "Win"];

function hotkeyName(event: KeyboardEvent<HTMLInputElement>): string | null {
  const modifier = { Control: "Ctrl", Alt: "Alt", Shift: "Shift", Meta: "Win" }[
    event.key
  ];
  if (modifier) return modifier;
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3);
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5);
  if (/^Numpad(?:[0-9]|Add|Subtract|Multiply|Divide|Decimal)$/.test(event.code)) {
    return event.code;
  }
  const physicalPunctuation: Record<string, string> = {
    Space: "Space",
    Equal: "Plus",
    Minus: "Minus",
    Comma: "Comma",
    Period: "Period",
    Slash: "Slash",
    Backslash: "Backslash",
    Semicolon: "Semicolon",
    Quote: "Quote",
    Backquote: "Backtick",
    BracketLeft: "BracketLeft",
    BracketRight: "BracketRight"
  };
  if (physicalPunctuation[event.code]) return physicalPunctuation[event.code];
  if (event.key.length === 1) {
    const punctuation: Record<string, string> = {
      " ": "Space",
      "+": "Plus",
      "-": "Minus",
      ",": "Comma",
      ".": "Period",
      "/": "Slash",
      "\\": "Backslash",
      ";": "Semicolon",
      "'": "Quote",
      "`": "Backtick",
      "[": "BracketLeft",
      "]": "BracketRight"
    };
    return punctuation[event.key] ?? event.key.toUpperCase();
  }
  if (event.key.startsWith("Arrow")) return event.key.slice(5);
  const supported = new Set([
    "Backspace",
    "Tab",
    "Enter",
    "Escape",
    "PageUp",
    "PageDown",
    "End",
    "Home",
    "Insert",
    "Delete",
    "CapsLock",
    "PrintScreen",
    "ScrollLock",
    "Pause"
  ]);
  if (supported.has(event.key) || /^F(?:[1-9]|1[0-9]|2[0-4])$/.test(event.key)) {
    return event.key;
  }
  return null;
}

function orderedHotkey(keys: string[]): string {
  const modifiers = hotkeyModifierOrder.filter((modifier) => keys.includes(modifier));
  const ordinary = keys.filter((key) => !hotkeyModifierOrder.includes(key));
  return [...modifiers, ...ordinary].join("+");
}

function HotkeyInput({
  value,
  onChange,
  onCaptureChange
}: {
  value: string;
  onChange: (value: string) => void;
  onCaptureChange: (active: boolean) => void;
}) {
  const [draft, setDraft] = useState(value);
  const pressed = useRef<string[]>([]);
  const pending = useRef<string | null>(null);
  const capturing = useRef(false);

  useEffect(() => {
    if (!capturing.current) setDraft(value);
  }, [value]);

  return (
    <input
      value={draft}
      readOnly
      spellCheck={false}
      aria-label="幽灵模式显隐快捷键"
      title="点击后同时按住全部按键；至少包含一个非修饰键"
      onFocus={(event) => {
        capturing.current = true;
        pressed.current = [];
        pending.current = null;
        onCaptureChange(true);
        event.currentTarget.select();
      }}
      onBlur={() => {
        capturing.current = false;
        pressed.current = [];
        onCaptureChange(false);
        if (pending.current) onChange(pending.current);
        setDraft(pending.current ?? value);
        pending.current = null;
      }}
      onKeyDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.repeat) return;
        const key = hotkeyName(event);
        if (!key) return;
        if (!pressed.current.includes(key)) pressed.current.push(key);
        const shortcut = orderedHotkey(pressed.current);
        setDraft(shortcut);
        if (pressed.current.some((pressedKey) => !hotkeyModifierOrder.includes(pressedKey))) {
          pending.current = shortcut;
        }
      }}
      onKeyUp={(event) => {
        event.preventDefault();
        event.stopPropagation();
        const key = hotkeyName(event);
        if (key) pressed.current = pressed.current.filter((pressedKey) => pressedKey !== key);
        if (pressed.current.length === 0 && pending.current) {
          onChange(pending.current);
          pending.current = null;
        }
      }}
    />
  );
}

function SettingSwitch({ title, detail, checked, onChange }: { title: string; detail: string; checked: boolean; onChange: (value: boolean) => void }) {
  return (
    <label className="setting-row switch-row">
      <div><b>{title}</b><span>{detail}</span></div>
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      <span className="switch"><span /></span>
    </label>
  );
}
