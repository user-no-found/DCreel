use std::{thread, time::Instant};
use tauri::{AppHandle, Manager, WebviewWindow, ipc::Channel};
use uuid::Uuid;

mod layout;
mod state;
mod window;
pub use state::{DesktopNotification, NotificationManager};
use state::{NOTIFICATION_DURATION, NOTIFICATION_WINDOW, TRANSFER_WINDOW};
pub use window::{hide_auxiliary_window, relayout_windows};
use window::{layout_windows, show_without_activation};

pub fn show_message(app: &AppHandle, title: impl Into<String>, message: impl Into<String>) {
    show(
        app,
        DesktopNotification {
            id: Uuid::new_v4().to_string(),
            kind: "message",
            title: title.into(),
            message: message.into(),
            version: None,
            action: None,
            shortcut_path: None,
        },
    );
}

pub fn show_update(app: &AppHandle, version: String) {
    show(
        app,
        DesktopNotification {
            id: Uuid::new_v4().to_string(),
            kind: "update",
            title: "DCreel 有新版本".into(),
            message: format!("版本 {version} 已经可以下载"),
            version: Some(version),
            action: None,
            shortcut_path: None,
        },
    );
}

fn show(app: &AppHandle, notification: DesktopNotification) {
    let ui_app = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        let Some(window) = ui_app.get_webview_window(NOTIFICATION_WINDOW) else {
            log::error!(target: "notification", "desktop notification window is unavailable");
            return;
        };
        let manager = ui_app.state::<NotificationManager>();
        let Ok(mut state) = manager.0.lock() else {
            return;
        };
        state.current = Some(notification.clone());
        state.deleting_id = None;
        state.shown_at = None;
        // Keep the new content until the WebView has subscribed and rendered it.
        // Never expose stale content while waiting for the render acknowledgement.
        let _ = hide_auxiliary_window(&window, "notification-replaced");
        if let Some(subscriber) = &state.subscriber
            && let Err(error) = subscriber.send(notification)
        {
            log::error!(target: "notification", "failed to send desktop notification: {error}");
        }
    }) {
        log::error!(target: "notification", "failed to queue desktop notification: {error}");
    }
}

#[tauri::command]
pub fn current_desktop_notification(
    manager: tauri::State<'_, NotificationManager>,
) -> Result<Option<DesktopNotification>, String> {
    Ok(manager
        .0
        .lock()
        .map_err(|_| "通知状态不可用")?
        .current
        .clone())
}

#[tauri::command]
pub fn subscribe_desktop_notifications(
    window: WebviewWindow,
    on_notification: Channel<DesktopNotification>,
) -> Result<(), String> {
    if window.label() != NOTIFICATION_WINDOW {
        return Err("不是通知窗口".into());
    }
    let manager = window.state::<NotificationManager>();
    let mut state = manager.0.lock().map_err(|_| "通知状态不可用")?;
    // Register and replay atomically. A channel is tied directly to this WebView,
    // including when the window is still being registered during app startup.
    state.shown_at = None;
    state.subscriber = Some(on_notification.clone());
    if let Some(current) = state.current.clone() {
        on_notification
            .send(current)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn present_desktop_notification(window: WebviewWindow, id: String) -> Result<(), String> {
    if window.label() != NOTIFICATION_WINDOW {
        return Err("不是通知窗口".into());
    }
    let manager = window.state::<NotificationManager>();
    let mut state = manager.0.lock().map_err(|_| "通知状态不可用")?;
    if !state.matches(&id) || state.shown_at.is_some() {
        return Ok(());
    }
    layout_windows(window.app_handle(), NOTIFICATION_WINDOW, true)?;
    show_without_activation(&window)?;
    let shown_at = Instant::now();
    state.shown_at = Some(shown_at);
    log::info!(target: "notification", "notification displayed id={id}");
    if state
        .current
        .as_ref()
        .is_some_and(|current| current.kind == "message")
    {
        schedule_dismissal(window.app_handle(), id, shown_at)?;
    }
    Ok(())
}

fn schedule_dismissal(app: &AppHandle, id: String, shown_at: Instant) -> Result<(), String> {
    let app = app.clone();
    thread::Builder::new().name("notification-timeout".into()).spawn(move || {
        thread::sleep(NOTIFICATION_DURATION);
        let ui_app = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            let manager = ui_app.state::<NotificationManager>();
            let Ok(mut state) = manager.0.lock() else { return; };
            if !state.can_expire(&id, shown_at) { return; }
            if let Some(window) = ui_app.get_webview_window(NOTIFICATION_WINDOW)
                && hide_auxiliary_window(&window, "notification-ended-9s").is_ok() {
                state.current = None;
                state.shown_at = None;
            }
        }) {
            log::error!(target: "notification", "failed to dispatch notification timeout: {error}");
        }
    }).map(|_| ()).map_err(|error| {
        log::error!(target: "notification", "failed to start notification timeout: {error}");
        error.to_string()
    })
}

pub fn dismiss_notification(
    window: &WebviewWindow,
    id: Option<&str>,
    reason: &str,
) -> Result<bool, String> {
    let manager = window.state::<NotificationManager>();
    let mut state = manager.0.lock().map_err(|_| "通知状态不可用")?;
    if id.is_some_and(|id| !state.matches(id)) {
        return Ok(false);
    }
    hide_auxiliary_window(window, reason)?;
    state.current = None;
    state.shown_at = None;
    Ok(true)
}

pub fn notification_page_loading(window: &WebviewWindow) {
    if window.label() != NOTIFICATION_WINDOW {
        return;
    }
    let manager = window.state::<NotificationManager>();
    if let Ok(mut state) = manager.0.lock() {
        state.shown_at = None;
        state.subscriber = None;
        let _ = hide_auxiliary_window(window, "notification-page-loading");
    }
}

pub fn show_transfer_progress(app: &AppHandle) {
    let ui_app = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if let Some(window) = ui_app.get_webview_window(TRANSFER_WINDOW)
            && let Err(error) = layout_windows(&ui_app, TRANSFER_WINDOW, true)
                .and_then(|_| show_without_activation(&window))
        {
            log::error!(target: "file_transfer", "failed to show transfer window: {error}");
        }
    }) {
        log::error!(target: "file_transfer", "failed to queue transfer window: {error}");
    }
}

pub fn show_broken_shortcut(app: &AppHandle, path: std::path::PathBuf, message: String) {
    show(
        app,
        DesktopNotification {
            id: Uuid::new_v4().to_string(),
            kind: "message",
            title: "DCreel 无法完成操作".into(),
            message,
            version: None,
            action: Some("deleteShortcut"),
            shortcut_path: Some(path),
        },
    );
}

#[tauri::command]
pub async fn delete_notification_shortcut(window: WebviewWindow, id: String) -> Result<(), String> {
    if window.label() != NOTIFICATION_WINDOW {
        return Err("不是通知窗口".into());
    }
    let path = window
        .state::<NotificationManager>()
        .0
        .lock()
        .map_err(|_| "通知状态不可用")?
        .claim_shortcut(&id)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        {
            creel_shell_operations::recycle_broken_shortcut(&path)
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Err("此操作仅支持 Windows".to_string())
        }
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|result| result);
    let ui_window = window.clone();
    let completed_id = id.clone();
    let succeeded = result.is_ok();
    window.app_handle().run_on_main_thread(move || {
        let manager = ui_window.state::<NotificationManager>();
        let Ok(mut state) = manager.0.lock() else { return; };
        if !state.complete_shortcut(&completed_id) { return; }
        drop(state);
        if succeeded {
            if let Err(error) = dismiss_notification(&ui_window, Some(&completed_id), "shortcut-recycled") {
                log::error!(target: "notification", "failed to dismiss shortcut notification: {error}");
            }
        }
    }).map_err(|error| error.to_string())?;
    if let Err(error) = &result {
        log::warn!(target: "notification", "shortcut deletion failed id={id}: {error}");
    }
    result
}
