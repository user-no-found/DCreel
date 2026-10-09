use super::{
    layout,
    state::{NOTIFICATION_WINDOW, TRANSFER_WINDOW},
};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

pub fn relayout_windows(app: &AppHandle, anchor: &str) {
    if let Err(error) = layout_windows(app, anchor, false) {
        log::warn!(target: "window", "failed to arrange auxiliary windows: {error}");
    }
}

pub(super) fn layout_windows(app: &AppHandle, incoming: &str, showing: bool) -> Result<(), String> {
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

pub(super) fn show_without_activation(window: &WebviewWindow) -> Result<(), String> {
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
