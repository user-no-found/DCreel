use crate::{
    commands::{ImportProgress, ImportStage, import_files_inner},
    desktop_notifications,
    store::AppStore,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

const PROGRESS_EVENT: &str = "creel://transfer-progress";
const PROGRESS_WINDOW: &str = "transfer-progress";
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(90);
const PROGRESS_WINDOW_DELAY: Duration = Duration::from_millis(420);
const PROGRESS_DISMISS_DELAY: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSnapshot {
    pub id: String,
    pub fence_title: String,
    pub phase: &'static str,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub total_items: u64,
    pub completed_items: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub can_cancel: bool,
}

impl TransferSnapshot {
    fn is_terminal(&self) -> bool {
        matches!(self.phase, "completed" | "cancelled" | "failed")
    }

    fn queued(id: String, fence_title: String) -> Self {
        Self {
            id,
            fence_title,
            phase: "queued",
            total_bytes: 0,
            completed_bytes: 0,
            total_items: 0,
            completed_items: 0,
            current_item: None,
            message: Some("正在准备移动…".into()),
            can_cancel: true,
        }
    }
}

struct TransferRequest {
    id: String,
    fence_id: String,
    fence_title: String,
    paths: Vec<PathBuf>,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
struct TransferState {
    snapshot: Option<TransferSnapshot>,
    active_cancel: Option<(String, Arc<AtomicBool>)>,
    dismissed_id: Option<String>,
}

#[derive(Clone, Copy, Debug)]
enum ProgressWindowAction {
    Show,
    Dismiss,
}

impl TransferState {
    fn allows_window_action(&self, id: &str, action: ProgressWindowAction) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.id == id
                && match action {
                    ProgressWindowAction::Show => {
                        !snapshot.is_terminal() && self.dismissed_id.as_deref() != Some(id)
                    }
                    ProgressWindowAction::Dismiss => snapshot.is_terminal(),
                }
        })
    }
}

pub struct FileTransferManager {
    sender: mpsc::Sender<TransferRequest>,
    state: Arc<Mutex<TransferState>>,
}

impl FileTransferManager {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel::<TransferRequest>();
        let state = Arc::new(Mutex::new(TransferState::default()));
        let worker_state = Arc::clone(&state);
        let worker_app = app.clone();
        thread::Builder::new()
            .name("file-transfer-worker".into())
            .spawn(move || worker_loop(worker_app, worker_state, receiver))
            .map_err(|error| format!("无法启动文件移动服务：{error}"))?;
        Ok(Self { sender, state })
    }

    pub fn enqueue(
        &self,
        fence_id: String,
        fence_title: String,
        paths: Vec<PathBuf>,
    ) -> Result<String, String> {
        let id = Uuid::new_v4().to_string();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.sender
            .send(TransferRequest {
                id: id.clone(),
                fence_id,
                fence_title,
                paths,
                cancelled,
            })
            .map_err(|_| "文件移动服务已停止".to_string())?;
        Ok(id)
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "文件移动状态暂时不可用".to_string())?;
        let Some((active_id, cancelled)) = state.active_cancel.as_ref() else {
            return Err("当前没有正在进行的文件移动".into());
        };
        if active_id != id {
            return Err("这项文件移动已经结束".into());
        }
        cancelled.store(true, Ordering::Release);
        log::info!(target: "file_transfer", "cancellation requested transfer_id={id}");
        Ok(())
    }

    pub fn snapshot(&self) -> Option<TransferSnapshot> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.snapshot.clone())
    }

    pub fn is_active(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.active_cancel.is_some())
            .unwrap_or(true)
    }

    pub fn dismiss_progress_window(&self, window: &tauri::WebviewWindow) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "文件移动状态暂时不可用".to_string())?;
        desktop_notifications::hide_auxiliary_window(window, "manual")?;
        state.dismissed_id = state.snapshot.as_ref().map(|snapshot| snapshot.id.clone());
        Ok(())
    }
}

fn worker_loop(
    app: AppHandle,
    state: Arc<Mutex<TransferState>>,
    receiver: mpsc::Receiver<TransferRequest>,
) {
    while let Ok(request) = receiver.recv() {
        let path_count = request.paths.len();
        let initial = TransferSnapshot::queued(request.id.clone(), request.fence_title.clone());
        if let Ok(mut state) = state.lock() {
            state.snapshot = Some(initial.clone());
            state.active_cancel = Some((request.id.clone(), Arc::clone(&request.cancelled)));
            state.dismissed_id = None;
        }
        emit_snapshot(&app, &initial);
        schedule_progress_window(&app, &state, &request.id, ProgressWindowAction::Show);
        log::info!(
            target: "file_transfer",
            "transfer started transfer_id={} fence_id={} path_count={path_count}",
            request.id,
            request.fence_id
        );

        let Some(store) = app.try_state::<AppStore>() else {
            finish_transfer(
                &app,
                &state,
                &request,
                "failed",
                "DCreel 状态尚未就绪".into(),
            );
            continue;
        };
        let mut last_emit = Instant::now() - PROGRESS_EMIT_INTERVAL;
        let mut latest = initial;
        let result = import_files_inner(
            &request.fence_id,
            request.paths.clone(),
            &app,
            &store,
            &request.cancelled,
            &mut |progress| {
                let next = snapshot_from_progress(&request, progress);
                let phase_changed = next.phase != latest.phase;
                latest = next;
                let now = Instant::now();
                if phase_changed || now.duration_since(last_emit) >= PROGRESS_EMIT_INTERVAL {
                    store_snapshot(&state, &latest);
                    emit_snapshot(&app, &latest);
                    last_emit = now;
                }
            },
        );

        match result {
            Ok(view) => {
                let mut completed = latest;
                completed.phase = "completed";
                completed.completed_bytes = completed.total_bytes;
                completed.completed_items = completed.total_items;
                completed.current_item = None;
                completed.message = Some(format!("已移入「{}」", view.config.title));
                completed.can_cancel = false;
                finish_with_snapshot(&app, &state, &request.id, completed);
                let progress_visible = app
                    .get_webview_window(PROGRESS_WINDOW)
                    .and_then(|window| window.is_visible().ok())
                    .unwrap_or(false);
                if !progress_visible {
                    desktop_notifications::show_message(
                        &app,
                        "文件移动完成",
                        format!("已移入「{}」", view.config.title),
                    );
                }
                log::info!(
                    target: "file_transfer",
                    "transfer completed transfer_id={} fence_id={} path_count={path_count}",
                    request.id,
                    request.fence_id
                );
            }
            Err(error) => {
                let cancelled = request.cancelled.load(Ordering::Acquire);
                let phase = if cancelled { "cancelled" } else { "failed" };
                let message = if cancelled {
                    "文件移动已取消；已经开始移动的项目已尝试移回原位置".to_string()
                } else {
                    format!("文件移动失败：{error}")
                };
                let mut failed = latest;
                failed.phase = phase;
                failed.current_item = None;
                failed.message = Some(message.clone());
                failed.can_cancel = false;
                finish_with_snapshot(&app, &state, &request.id, failed);
                let _ = app.emit("creel://notification", &message);
                desktop_notifications::show_message(&app, "DCreel 文件移动", message.clone());
                if cancelled {
                    log::info!(
                        target: "file_transfer",
                        "transfer cancelled transfer_id={} fence_id={}",
                        request.id,
                        request.fence_id
                    );
                } else {
                    log::error!(
                        target: "file_transfer",
                        "transfer failed transfer_id={} fence_id={}: {error}",
                        request.id,
                        request.fence_id
                    );
                }
            }
        }
    }
    log::error!(target: "file_transfer", "file transfer request channel closed");
}

fn schedule_progress_window(
    app: &AppHandle,
    state: &Arc<Mutex<TransferState>>,
    transfer_id: &str,
    action: ProgressWindowAction,
) {
    let delay = match action {
        ProgressWindowAction::Show => PROGRESS_WINDOW_DELAY,
        ProgressWindowAction::Dismiss => PROGRESS_DISMISS_DELAY,
    };
    let app = app.clone();
    let state = Arc::clone(state);
    let transfer_id = transfer_id.to_string();
    if let Err(error) = thread::Builder::new()
        .name("transfer-window-timer".into())
        .spawn(move || {
            thread::sleep(delay);
            let ui_app = app.clone();
            // Check the task on the UI thread immediately before changing visibility.
            // A previous task's timeout must never hide the next task's window.
            if let Err(error) = app.run_on_main_thread(move || {
                let Ok(state) = state.lock() else {
                    log::error!(target: "file_transfer", "transfer window timer state unavailable");
                    return;
                };
                if !state.allows_window_action(&transfer_id, action) {
                    return;
                }
                match action {
                    ProgressWindowAction::Show => {
                        desktop_notifications::show_transfer_progress(&ui_app);
                    }
                    ProgressWindowAction::Dismiss => {
                        if let Some(window) = ui_app.get_webview_window(PROGRESS_WINDOW) {
                            let _ = desktop_notifications::hide_auxiliary_window(
                                &window,
                                "transfer-ended-3s",
                            );
                        }
                    }
                }
            }) {
                log::error!(target: "file_transfer", "failed to dispatch transfer window timer action={action:?}: {error}");
            }
        })
    {
        log::error!(target: "file_transfer", "failed to start transfer window timer action={action:?}: {error}");
    }
}

fn snapshot_from_progress(request: &TransferRequest, progress: ImportProgress) -> TransferSnapshot {
    let phase = match progress.stage {
        ImportStage::Preparing => "preparing",
        ImportStage::Moving => "moving",
        ImportStage::Finalizing => "finalizing",
        ImportStage::RollingBack => "rolling_back",
    };
    let current_item = progress.current_item.as_deref().and_then(display_item_name);
    let message = Some(match progress.stage {
        ImportStage::Preparing => "正在统计文件…".into(),
        ImportStage::Moving => "正在移动到映射文件夹…".into(),
        ImportStage::Finalizing => "内容已复制，正在完成移动…".into(),
        ImportStage::RollingBack => "移动未完成，正在安全撤销…".into(),
    });
    TransferSnapshot {
        id: request.id.clone(),
        fence_title: request.fence_title.clone(),
        phase,
        total_bytes: progress.total_bytes,
        completed_bytes: progress.completed_bytes,
        total_items: progress.total_items,
        completed_items: progress.completed_items,
        current_item,
        message,
        can_cancel: !matches!(
            progress.stage,
            ImportStage::Finalizing | ImportStage::RollingBack
        ),
    }
}

fn display_item_name(path: &std::path::Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
}

fn store_snapshot(state: &Arc<Mutex<TransferState>>, snapshot: &TransferSnapshot) {
    if let Ok(mut state) = state.lock() {
        state.snapshot = Some(snapshot.clone());
    }
}

fn emit_snapshot(app: &AppHandle, snapshot: &TransferSnapshot) {
    if let Err(error) = app.emit_to(PROGRESS_WINDOW, PROGRESS_EVENT, snapshot) {
        log::warn!(target: "file_transfer", "failed to emit transfer progress: {error}");
    }
}

fn finish_with_snapshot(
    app: &AppHandle,
    state: &Arc<Mutex<TransferState>>,
    id: &str,
    snapshot: TransferSnapshot,
) {
    if let Ok(mut state) = state.lock() {
        state.snapshot = Some(snapshot.clone());
        if state
            .active_cancel
            .as_ref()
            .is_some_and(|(active_id, _)| active_id == id)
        {
            state.active_cancel = None;
        }
    }
    emit_snapshot(app, &snapshot);
    schedule_progress_window(app, state, id, ProgressWindowAction::Dismiss);
}

fn finish_transfer(
    app: &AppHandle,
    state: &Arc<Mutex<TransferState>>,
    request: &TransferRequest,
    phase: &'static str,
    message: String,
) {
    let mut snapshot = TransferSnapshot::queued(request.id.clone(), request.fence_title.clone());
    snapshot.phase = phase;
    snapshot.message = Some(message);
    snapshot.can_cancel = false;
    finish_with_snapshot(app, state, &request.id, snapshot);
}

#[tauri::command]
pub fn cancel_file_transfer(
    id: String,
    transfers: tauri::State<'_, FileTransferManager>,
) -> Result<(), String> {
    transfers.cancel(&id)
}

#[tauri::command]
pub fn current_file_transfer(
    transfers: tauri::State<'_, FileTransferManager>,
) -> Option<TransferSnapshot> {
    transfers.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_phase(phase: &'static str) -> TransferState {
        let mut snapshot = TransferSnapshot::queued("task-1".into(), "test".into());
        snapshot.phase = phase;
        TransferState {
            snapshot: Some(snapshot),
            ..Default::default()
        }
    }

    #[test]
    fn terminal_tasks_dismiss_but_never_show_from_a_delayed_timer() {
        for phase in ["completed", "cancelled", "failed"] {
            let state = state_with_phase(phase);
            assert!(state.allows_window_action("task-1", ProgressWindowAction::Dismiss));
            assert!(!state.allows_window_action("task-1", ProgressWindowAction::Show));
        }
    }

    #[test]
    fn active_tasks_are_not_auto_dismissed() {
        for phase in [
            "queued",
            "preparing",
            "moving",
            "finalizing",
            "rolling_back",
        ] {
            let state = state_with_phase(phase);
            assert!(state.allows_window_action("task-1", ProgressWindowAction::Show));
            assert!(!state.allows_window_action("task-1", ProgressWindowAction::Dismiss));
        }
    }

    #[test]
    fn old_timers_cannot_change_the_next_tasks_window() {
        for phase in ["moving", "completed"] {
            let state = state_with_phase(phase);
            assert!(!state.allows_window_action("old-task", ProgressWindowAction::Show));
            assert!(!state.allows_window_action("old-task", ProgressWindowAction::Dismiss));
        }
    }

    #[test]
    fn manual_dismissal_prevents_a_delayed_reopen() {
        let mut state = state_with_phase("moving");
        state.dismissed_id = Some("task-1".into());
        assert!(!state.allows_window_action("task-1", ProgressWindowAction::Show));
        state.snapshot.as_mut().unwrap().id = "task-2".into();
        assert!(state.allows_window_action("task-2", ProgressWindowAction::Show));
    }

    #[test]
    fn empty_state_does_not_show_or_dismiss() {
        let state = TransferState::default();
        assert!(!state.allows_window_action("task-1", ProgressWindowAction::Show));
        assert!(!state.allows_window_action("task-1", ProgressWindowAction::Dismiss));
        assert_eq!(PROGRESS_DISMISS_DELAY, Duration::from_secs(3));
    }
}

#[cfg(all(test, windows))]
#[path = "file_transfer_window_test.rs"]
mod window_test;
