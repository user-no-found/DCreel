use crate::{desktop_host::DesktopHostController, models::FenceConfig, store::AppStore};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{AppHandle, Emitter, Manager};

mod event_dispatcher;
use event_dispatcher::{EventDispatcher, EventSink};

struct WatchEntry {
    directory: PathBuf,
    _watcher: RecommendedWatcher,
}

#[derive(Default)]
pub struct DirectoryWatchers {
    entries: Mutex<HashMap<String, WatchEntry>>,
    dispatcher: Mutex<Option<EventDispatcher>>,
    stopped: AtomicBool,
}

impl DirectoryWatchers {
    pub fn sync(&self, app: &AppHandle, fences: &[FenceConfig]) -> Result<(), String> {
        let Some(sink) = self.event_sink(app)? else {
            return Ok(());
        };
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "目录监听状态暂时不可用".to_string())?;
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
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
            let fence_id = fence.id.clone();
            let callback_id = fence_id.clone();
            let callback_sink = sink.clone();
            let Ok(mut watcher) = RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| {
                    let Ok(event) = event else {
                        return;
                    };
                    if matches!(event.kind, EventKind::Access(_)) {
                        return;
                    }
                    callback_sink.enqueue(&callback_id);
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

    fn event_sink(&self, app: &AppHandle) -> Result<Option<EventSink>, String> {
        let mut dispatcher = self
            .dispatcher
            .lock()
            .map_err(|_| "目录事件处理状态暂时不可用".to_string())?;
        if self.stopped.load(Ordering::Acquire) {
            return Ok(None);
        }
        if dispatcher.is_none() {
            let app = app.clone();
            *dispatcher = Some(EventDispatcher::new(move |id| {
                if let Some(host) = app.try_state::<DesktopHostController>()
                    && let Err(error) = host.refresh_fence(id)
                {
                    log::warn!(target: "directory_watcher", "directory refresh failed id={id}: {error}");
                }
                let _ = app.emit("creel://folder-changed", id);
            })?);
        }
        Ok(dispatcher.as_ref().map(EventDispatcher::sink))
    }

    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(mut dispatcher) = self.dispatcher.lock() {
            dispatcher.take();
        }
        // Drop native watchers outside the registry lock.
        if let Ok(mut entries) = self.entries.lock() {
            let removed = std::mem::take(&mut *entries);
            drop(entries);
            drop(removed);
        }
    }
}

impl Drop for DirectoryWatchers {
    fn drop(&mut self) {
        self.shutdown();
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
