import { useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { AlertCircle, Download, X } from "lucide-react";
import {
  subscribeDesktopNotifications,
  presentDesktopNotification,
  dismissAuxiliaryWindow,
  deleteNotificationShortcut,
  showUpdateDetails,
  ignoreUpdateVersion,
  writeFrontendLog
} from "../lib/bridge";
import type { DesktopNotificationPayload } from "../types";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function DesktopNotificationWindow() {
  const currentId = useRef<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [notification, setNotification] = useState<DesktopNotificationPayload | null>(null);

  useLayoutEffect(() => {
    let disposed = false;
    void subscribeDesktopNotifications((payload) => {
      if (disposed) return;
      currentId.current = payload.id;
      flushSync(() => {
        setNotification(payload);
        setDeleting(false);
        setDeleteError(null);
      });
    }).catch((error) => {
      void writeFrontendLog("error", `通知监听失败：${errorMessage(error)}`, "notification").catch(() => undefined);
    });
    return () => { disposed = true; currentId.current = null; };
  }, []);

  useLayoutEffect(() => {
    // Hidden WebViews can postpone paint/passive effects. A layout effect runs
    // immediately after the DOM commit, without requiring the window to show first.
    if (notification) void presentDesktopNotification(notification.id).catch(() => undefined);
  }, [notification?.id]);

  const deleteShortcut = async (id: string) => {
    setDeleting(true);
    setDeleteError(null);
    try {
      await deleteNotificationShortcut(id);
    } catch (error) {
      if (currentId.current === id) setDeleteError(errorMessage(error));
    } finally {
      if (currentId.current === id) setDeleting(false);
    }
  };

  if (!notification) return null;
  const isUpdate = notification.kind === "update" && Boolean(notification.version);
  const hasShortcutAction = notification.action === "deleteShortcut";
  return (
    <section
      className={`desktop-notification${hasShortcutAction ? " has-shortcut-action" : ""}`}
      role="status"
      aria-live="polite"
    >
      <div className={`desktop-notification-icon ${isUpdate ? "update" : "warning"}`}>
        {isUpdate ? <Download /> : <AlertCircle />}
      </div>
      <div className="desktop-notification-copy">
        <b>{notification.title}</b>
        <p className="desktop-notification-message">{notification.message}</p>
        {deleteError && <p role="alert" className="notification-action-error">{deleteError}</p>}
        {hasShortcutAction && (
          <div className="desktop-notification-actions">
            <button className="notification-primary" disabled={deleting}
              onClick={() => void deleteShortcut(notification.id)}>
              {deleting ? "正在删除…" : "删除快捷方式"}
            </button>
          </div>
        )}
        {isUpdate && (
          <div className="desktop-notification-actions">
            <button
              className="notification-primary"
              onClick={() => void showUpdateDetails(notification.id).catch(() => undefined)}
            >
              查看更新
            </button>
            <button
              onClick={() => {
                const version = notification.version;
                if (!version) return;
                void ignoreUpdateVersion(version)
                  .then(() => dismissAuxiliaryWindow(notification.id))
                  .catch(() => undefined);
              }}
            >
              不再提醒此版本
            </button>
          </div>
        )}
      </div>
      <button
        className="desktop-notification-close"
        aria-label="关闭通知"
        title="关闭通知"
        onClick={() => void dismissAuxiliaryWindow(notification.id).catch(() => undefined)}
      >
        <X />
      </button>
    </section>
  );
}
