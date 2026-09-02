use crate::{desktop_host::DesktopHostController, models::FenceConfig, store::AppStore};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Mutex,
};
use tauri::{AppHandle, Emitter, Manager};

struct WatchEntry {
    directory: PathBuf,
    _watcher: RecommendedWatcher,
}

#[derive(Default)]
pub struct DirectoryWatchers {
    entries: Mutex<HashMap<String, WatchEntry>>,
}

impl DirectoryWatchers {
    pub fn sync(&self, app: &AppHandle, fences: &[FenceConfig]) -> Result<(), String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "目录监听状态暂时不可用".to_string())?;
        let desired: HashSet<&str> = fences.iter().map(|fence| fence.id.as_str()).collect();
        entries.retain(|id, entry| {
            desired.contains(id.as_str())
                && fences
                    .iter()
                    .find(|fence| fence.id == *id)
                    .is_some_and(|fence| fence.directory == entry.directory)
        });

        for fence in fences {
            if entries.contains_key(&fence.id) || !fence.directory.is_dir() {
                continue;
            }
            let app_handle = app.clone();
            let fence_id = fence.id.clone();
            let callback_id = fence_id.clone();
            let Ok(mut watcher) = RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| {
                    let Ok(event) = event else {
                        return;
                    };
                    if matches!(event.kind, EventKind::Access(_)) {
                        return;
                    }
                    #[cfg(debug_assertions)]
                    eprintln!("[dcreel] folder changed: {callback_id} ({:?})", event.kind);
                    if let Some(host) = app_handle.try_state::<DesktopHostController>() {
                        let _ = host.refresh_fence(&callback_id);
                    }
                    let _ = app_handle.emit("creel://folder-changed", &callback_id);
                },
                Config::default(),
            ) else {
                continue;
            };
            if watcher
                .watch(&fence.directory, RecursiveMode::NonRecursive)
                .is_err()
            {
                continue;
            }
            entries.insert(
                fence_id,
                WatchEntry {
                    directory: fence.directory.clone(),
                    _watcher: watcher,
                },
            );
        }
        Ok(())
    }
}

pub fn sync_from_store(app: &AppHandle) -> Result<(), String> {
    let store = app.state::<AppStore>();
    let fences = store
        .lock()
        .map_err(|error| error.to_string())?
        .fences
        .clone();
    let watchers = app.state::<DirectoryWatchers>();
    watchers.sync(app, &fences)
}
