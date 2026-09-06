use serde::Serialize;
use std::{
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, ipc::Channel};
use uuid::Uuid;

mod layout;

const NOTIFICATION_WINDOW: &str = "desktop-notification";
const TRANSFER_WINDOW: &str = "transfer-progress";
const NOTIFICATION_DURATION: Duration = Duration::from_secs(9);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopNotification {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Default)]
pub struct NotificationManager(Mutex<NotificationState>);

#[derive(Default)]
struct NotificationState {
    current: Option<DesktopNotification>,
    shown_at: Option<Instant>,
    subscriber: Option<Channel<DesktopNotification>>,
}

impl NotificationState {
    fn matches(&self, id: &str) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.id == id)
    }

    fn can_expire(&self, id: &str, shown_at: Instant) -> bool {
        self.matches(id)
            && self.shown_at == Some(shown_at)
            && self
                .current
                .as_ref()
                .is_some_and(|current| current.kind == "message")
    }
}

pub fn show_message(app: &AppHandle, title: impl Into<String>, message: impl Into<String>) {
    show(
        app,
        DesktopNotification {
            id: Uuid::new_v4().to_string(),
            kind: "message",
            title: title.into(),
            message: message.into(),
            version: None,
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

pub fn relayout_windows(app: &AppHandle, anchor: &str) {
    if let Err(error) = layout_windows(app, anchor, false) {
        log::warn!(target: "window", "failed to arrange auxiliary windows: {error}");
    }
}

fn layout_windows(app: &AppHandle, incoming: &str, showing: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::{HWND, POINT},
            Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
                MonitorFromWindow,
            },
            UI::WindowsAndMessaging::GetCursorPos,
        };

        let transfer = app.get_webview_window(TRANSFER_WINDOW);
        let notification = app.get_webview_window(NOTIFICATION_WINDOW);
        let visible = |window: &WebviewWindow| window.is_visible().unwrap_or(false);
        let active =
            |window: &WebviewWindow| visible(window) || (showing && window.label() == incoming);
        let windows: Vec<_> = [transfer, notification]
            .into_iter()
            .flatten()
            .filter(active)
            .collect();
        if windows.is_empty() {
            return Ok(());
        }
        // Keep an existing popup's monitor when adding its companion.
        let existing = windows
            .iter()
            .find(|window| visible(window) && (!showing || window.label() != incoming));
        let anchor = existing.unwrap_or(&windows[0]);
        let hwnd = HWND(anchor.hwnd().map_err(|error| error.to_string())?.0);
        let mut cursor = POINT::default();
        let monitor =
            if showing && existing.is_none() && unsafe { GetCursorPos(&mut cursor) }.is_ok() {
                unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) }
            } else {
                unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }
            };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let scale = anchor.scale_factor().map_err(|error| error.to_string())?;
        let sizes: Vec<_> = windows
            .iter()
            .map(|window| {
                let scale = window.scale_factor().unwrap_or(scale);
                let config = app
                    .config()
                    .app
                    .windows
                    .iter()
                    .find(|config| config.label == window.label());
                let actual = window.outer_size().map_err(|error| error.to_string())?;
                // Restore the configured logical size after a small work area
                // forced a narrower popup, then clamp the complete arrangement.
                let (width, height) = config
                    .map(|config| (config.width * scale, config.height * scale))
                    .unwrap_or((f64::from(actual.width), f64::from(actual.height)));
                Ok((width.round() as i32, height.round() as i32))
            })
            .collect::<Result<_, String>>()?;
        let work = layout::Rect {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        };
        let rectangles = layout::arrange(work, &sizes, (18.0 * scale).round() as i32);
        for (window, rect) in windows.iter().zip(rectangles) {
            let size = PhysicalSize::new(
                (rect.right - rect.left) as u32,
                (rect.bottom - rect.top) as u32,
            );
            let position = PhysicalPosition::new(rect.left, rect.top);
            if window.outer_size().map_err(|error| error.to_string())? != size {
                window.set_size(size).map_err(|error| error.to_string())?;
            }
            if window.outer_position().map_err(|error| error.to_string())? != position {
                window
                    .set_position(position)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = showing;
        app.get_webview_window(incoming)
            .ok_or("窗口不存在")?
            .center()
            .map_err(|error| error.to_string())
    }
}

fn show_without_activation(window: &WebviewWindow) -> Result<(), String> {
    // Both auxiliary windows use focus:false and alwaysOnTop:true in the config.
    // Tao honors that without activation. Do not call Win32 ShowWindow here:
    // it bypasses Tao's VISIBLE flag, making the later hide() a no-op.
    window.show().map_err(|error| error.to_string())
}

pub fn hide_auxiliary_window(window: &WebviewWindow, reason: &str) -> Result<(), String> {
    let label = window.label();
    if !matches!(label, NOTIFICATION_WINDOW | TRANSFER_WINDOW) {
        return Err(format!("窗口 {label} 不能通过辅助窗口命令关闭"));
    }
    let result = (|| {
        window.hide().map_err(|error| error.to_string())?;
        // Check the actual HWND visibility, not just whether Hide was queued.
        if window.is_visible().map_err(|error| error.to_string())? {
            return Err("关闭请求执行后窗口仍然可见".to_string());
        }
        Ok(())
    })();
    match &result {
        Ok(()) => {
            log::info!(target: "window", "auxiliary window hidden label={label} reason={reason}");
            relayout_windows(window.app_handle(), label);
        }
        Err(error) => {
            log::error!(target: "window", "failed to hide auxiliary window label={label} reason={reason}: {error}")
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeout_is_bound_to_notification_and_render_generation() {
        let time = Instant::now();
        let mut state = NotificationState {
            current: Some(DesktopNotification {
                id: "new".into(),
                kind: "message",
                title: String::new(),
                message: String::new(),
                version: None,
            }),
            shown_at: Some(time),
            subscriber: None,
        };
        assert!(!state.can_expire("old", time));
        assert!(state.can_expire("new", time));
        state.shown_at = None;
        assert!(!state.can_expire("new", time));
        state.shown_at = Some(time + Duration::from_secs(1));
        assert!(!state.can_expire("new", time));
        state.current.as_mut().unwrap().kind = "update";
        assert!(!state.can_expire("new", state.shown_at.unwrap()));
    }
}
