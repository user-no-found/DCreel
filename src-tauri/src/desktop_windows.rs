use crate::{desktop_host::DesktopHostController, directory_watchers, store::AppStore};
use tauri::{AppHandle, Emitter, Manager};

const STATE_EVENT: &str = "creel://state-changed";

pub fn sync_all(app: &AppHandle) -> Result<(), String> {
    let store = app.state::<AppStore>();
    let state = store.lock().map_err(|error| error.to_string())?.clone();
    let should_show = state.preferences.desktop_mode && store.desktop_visible();

    let host_result = match app.try_state::<DesktopHostController>() {
        Some(host) if host.is_active() => host.sync(&state, should_show),
        _ => Err("纯 Rust Desktop Host 在当前平台不可用".to_string()),
    };
    let watcher_result = directory_watchers::sync_from_store(app);
    match (host_result, watcher_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(host), Ok(())) => Err(format!("Desktop Host 同步失败：{host}")),
        (Ok(()), Err(watcher)) => Err(format!("目录监听同步失败：{watcher}")),
        (Err(host), Err(watcher)) => Err(format!(
            "Desktop Host 同步失败：{host}；目录监听同步失败：{watcher}"
        )),
    }
}

pub fn toggle_visibility(app: &AppHandle) -> Result<bool, String> {
    let store = app.state::<AppStore>();
    let visible = !store.desktop_visible();
    store.set_desktop_visible(visible);
    let _ = app.emit(STATE_EVENT, ());
    if let Some(host) = app.try_state::<DesktopHostController>() {
        host.request_sync();
    }
    Ok(visible)
}
