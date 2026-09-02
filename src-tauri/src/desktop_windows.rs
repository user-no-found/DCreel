use crate::{desktop_host::DesktopHostController, directory_watchers, store::AppStore};
use tauri::{AppHandle, Emitter, Manager};

const STATE_EVENT: &str = "creel://state-changed";

pub fn sync_all(app: &AppHandle) -> Result<(), String> {
    let store = app.state::<AppStore>();
    let state = store.lock().map_err(|error| error.to_string())?.clone();
    let should_show = state.preferences.desktop_mode && store.desktop_visible();

    let host = app
        .try_state::<DesktopHostController>()
        .filter(|host| host.is_active())
        .ok_or_else(|| "纯 Rust Desktop Host 在当前平台不可用".to_string())?;
    host.sync(&state, should_show)?;
    directory_watchers::sync_from_store(app)?;
    let _ = app.emit(STATE_EVENT, ());
    Ok(())
}

pub fn close_fence(app: &AppHandle, _id: &str) -> Result<(), String> {
    sync_all(app)
}

pub fn toggle_visibility(app: &AppHandle) -> Result<bool, String> {
    let store = app.state::<AppStore>();
    let visible = !store.desktop_visible();
    store.set_desktop_visible(visible);
    sync_all(app)?;
    Ok(visible)
}
