use crate::{
    desktop_notifications, file_transfers::FileTransferManager, models::PersistedState,
    store::AppStore,
};
use tauri::AppHandle;

#[cfg(windows)]
mod platform {
    use super::*;
    use creel_ipc::{
        HostCommand, HostEvent, HostFenceSnapshot, HostPreferencesSnapshot, HostUserAction,
        PROTOCOL_VERSION, message_line,
    };
    use std::{
        io::{BufRead, BufReader, Write},
        os::windows::process::CommandExt,
        path::{Path, PathBuf},
        process::{Child, ChildStdin, Command, ExitStatus, Stdio},
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };
    use tauri::{Emitter, Manager};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub struct DesktopHostController {
        inner: Mutex<ControllerState>,
        app: AppHandle,
        supervisor_signal: Arc<SupervisorSignal>,
        supervisor: Mutex<Option<thread::JoinHandle<()>>>,
    }

    struct SupervisorSignal {
        shutting_down: AtomicBool,
        sync_requested: AtomicBool,
        wake_lock: Mutex<()>,
        wake: Condvar,
    }

    struct ControllerState {
        executable: Option<PathBuf>,
        running: Option<RunningHost>,
        next_revision: u64,
    }

    struct RunningHost {
        child: Child,
        stdin: ChildStdin,
        events: mpsc::Receiver<Result<HostEvent, String>>,
    }

    enum SyncFailure {
        Host(String),
        Transport(String),
    }

    impl std::fmt::Display for SyncFailure {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Host(message) | Self::Transport(message) => formatter.write_str(message),
            }
        }
    }

    enum NativeRequest {
        UserAction(HostUserAction),
    }

    impl DesktopHostController {
        pub fn new(app: &AppHandle) -> Self {
            let executable = match find_executable(app) {
                Ok(executable) => Some(executable),
                Err(error) => {
                    log::error!(target: "desktop_host", "host executable discovery failed: {error}");
                    None
                }
            };
            Self {
                inner: Mutex::new(ControllerState {
                    executable,
                    running: None,
                    next_revision: 1,
                }),
                app: app.clone(),
                supervisor_signal: Arc::new(SupervisorSignal {
                    shutting_down: AtomicBool::new(false),
                    sync_requested: AtomicBool::new(true),
                    wake_lock: Mutex::new(()),
                    wake: Condvar::new(),
                }),
                supervisor: Mutex::new(None),
            }
        }

        pub fn start_supervisor(&self) -> Result<(), String> {
            let mut supervisor = self
                .supervisor
                .lock()
                .map_err(|_| "Desktop Host 监督线程状态不可用".to_string())?;
            if supervisor.is_some() {
                return Ok(());
            }
            let app = self.app.clone();
            let signal = Arc::clone(&self.supervisor_signal);
            let handle = thread::Builder::new()
                .name("desktop-host-supervisor".into())
                .spawn(move || supervisor_loop(app, signal))
                .map_err(|error| format!("无法启动 Desktop Host 监督线程：{error}"))?;
            *supervisor = Some(handle);
            log::info!(target: "desktop_host", "desktop supervisor started");
            Ok(())
        }

        pub fn request_sync(&self) {
            if self.supervisor_signal.shutting_down.load(Ordering::Acquire) {
                return;
            }
            self.supervisor_signal
                .sync_requested
                .store(true, Ordering::Release);
            self.supervisor_signal.wake.notify_one();
        }

        pub fn is_active(&self) -> bool {
            self.inner
                .lock()
                .map(|state| state.executable.is_some())
                .unwrap_or(false)
        }

        pub fn sync(&self, state: &PersistedState, visible: bool) -> Result<(), String> {
            if self.supervisor_signal.shutting_down.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut controller = self
                .inner
                .lock()
                .map_err(|_| "Desktop Host 控制状态不可用".to_string())?;
            let Some(executable) = controller.executable.clone() else {
                return Ok(());
            };
            let revision = controller.next_revision;
            let command = HostCommand::Sync {
                revision,
                fences: state
                    .fences
                    .iter()
                    .map(|fence| HostFenceSnapshot {
                        id: fence.id.clone(),
                        title: fence.title.clone(),
                        directory: fence.directory.clone(),
                        x: fence.x,
                        y: fence.y,
                        width: fence.width,
                        height: fence.height,
                        color: fence.color.clone(),
                        content_color: fence.content_color.clone(),
                        collapsed: fence.collapsed,
                        locked: fence.locked,
                        display_anchor: fence.display_anchor.clone(),
                        placement: fence.placement.clone(),
                    })
                    .collect(),
                preferences: HostPreferencesSnapshot {
                    title_opacity: state.preferences.title_opacity,
                    content_opacity: state.preferences.content_opacity,
                    show_fence_border: state.preferences.show_fence_border,
                    fence_border_opacity: state.preferences.fence_border_opacity,
                    icon_size: state.preferences.icon_size,
                    ghost_mode: state.preferences.ghost_mode,
                    ghost_mode_trigger: state.preferences.ghost_mode_trigger,
                    ghost_opacity: state.preferences.ghost_opacity,
                    ghost_hotkey: state.preferences.ghost_hotkey.clone(),
                    show_hidden_files: state.preferences.show_hidden_files,
                    show_fence_titles: state.preferences.show_fence_titles,
                },
                visible,
            };
            controller.next_revision = controller.next_revision.wrapping_add(1).max(1);
            log::debug!(
                target: "desktop_host",
                "sending sync revision={revision} fences={} visible={visible}",
                state.fences.len()
            );
            if let Err(first_error) = send_sync(&mut controller, &executable, &self.app, &command) {
                let first_error = match first_error {
                    SyncFailure::Host(message) => {
                        // Host 能正常回传结构化错误，说明 IPC 和进程仍然健康。
                        // Explorer 启动阶段的桌面层拒绝访问通常是暂时的，无需
                        // 反复销毁 Host；监督线程会用同一进程快速重试。
                        log::warn!(
                            target: "desktop_host",
                            "sync revision={revision} was rejected by host and will retry: {message}"
                        );
                        return Err(message);
                    }
                    SyncFailure::Transport(message) => message,
                };
                log::warn!(
                    target: "desktop_host",
                    "sync revision={revision} transport failed; restarting host: {first_error}"
                );
                stop_running(controller.running.take());
                controller.running = Some(spawn_host(&executable, &self.app)?);
                if let Err(second_error) =
                    send_sync(&mut controller, &executable, &self.app, &command)
                {
                    log::error!(
                        target: "desktop_host",
                        "sync revision={revision} failed after transport restart: {second_error}"
                    );
                    stop_running(controller.running.take());
                    return Err(second_error.to_string());
                }
            }
            log::info!(
                target: "desktop_host",
                "sync completed revision={revision} fences={} visible={visible}",
                state.fences.len()
            );
            Ok(())
        }

        pub fn shutdown(&self) {
            if self
                .supervisor_signal
                .shutting_down
                .swap(true, Ordering::AcqRel)
            {
                return;
            }
            log::info!(target: "desktop_host", "desktop integration shutdown started");
            self.supervisor_signal.wake.notify_all();
            if let Ok(mut supervisor) = self.supervisor.lock()
                && let Some(handle) = supervisor.take()
                && handle.join().is_err()
            {
                log::error!(target: "desktop_host", "desktop supervisor thread panicked");
            }
            if let Ok(mut controller) = self.inner.lock() {
                stop_running(controller.running.take());
            }
            log::info!(target: "desktop_host", "desktop integration shutdown completed");
        }

        pub fn refresh_fence(&self, id: &str) -> Result<(), String> {
            let mut controller = self
                .inner
                .lock()
                .map_err(|_| "Desktop Host 控制状态不可用".to_string())?;
            let Some(running) = controller.running.as_mut() else {
                return Ok(());
            };
            if running
                .child
                .try_wait()
                .map_err(|error| format!("无法检查 Desktop Host 状态：{error}"))?
                .is_some()
            {
                return Ok(());
            }
            send_command(
                &mut running.stdin,
                &HostCommand::RefreshFence { id: id.into() },
            )
        }

        pub fn set_hotkey_capture_active(&self, active: bool) -> Result<(), String> {
            let mut controller = self
                .inner
                .lock()
                .map_err(|_| "Desktop Host 控制状态不可用".to_string())?;
            let Some(running) = controller.running.as_mut() else {
                return Ok(());
            };
            if running
                .child
                .try_wait()
                .map_err(|error| format!("无法检查 Desktop Host 状态：{error}"))?
                .is_some()
            {
                return Ok(());
            }
            send_command(
                &mut running.stdin,
                &HostCommand::SetHotkeyCapture { active },
            )
        }

        fn host_requires_sync(&self) -> bool {
            let Ok(mut controller) = self.inner.lock() else {
                log::error!(target: "desktop_host", "host state lock is poisoned");
                return true;
            };
            if controller.executable.is_none() {
                return false;
            }
            let status = match controller.running.as_mut() {
                Some(running) => match running.child.try_wait() {
                    Ok(status) => status,
                    Err(error) => {
                        log::error!(target: "desktop_host", "failed to inspect host process: {error}");
                        stop_running(controller.running.take());
                        return true;
                    }
                },
                None => return true,
            };
            if let Some(status) = status {
                log_host_exit(status);
                stop_running(controller.running.take());
                return true;
            }
            false
        }
    }

    impl Drop for DesktopHostController {
        fn drop(&mut self) {
            self.supervisor_signal
                .shutting_down
                .store(true, Ordering::Release);
            self.supervisor_signal.wake.notify_all();
            if let Ok(supervisor) = self.supervisor.get_mut()
                && let Some(handle) = supervisor.take()
            {
                let _ = handle.join();
            }
            if let Ok(controller) = self.inner.get_mut() {
                stop_running(controller.running.take());
            }
        }
    }

    fn supervisor_loop(app: AppHandle, signal: Arc<SupervisorSignal>) {
        const RETRY_DELAYS: [Duration; 7] = [
            Duration::from_millis(250),
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(3),
            Duration::from_secs(5),
        ];
        const MONITOR_INTERVAL: Duration = Duration::from_secs(5);

        let mut failures = 0usize;
        while !signal.shutting_down.load(Ordering::Acquire) {
            let requested = signal.sync_requested.swap(false, Ordering::AcqRel);
            let needs_recovery = app
                .try_state::<DesktopHostController>()
                .is_some_and(|controller| controller.host_requires_sync());
            let wait_duration = if requested || needs_recovery {
                match crate::desktop_windows::sync_all(&app) {
                    Ok(()) => {
                        if failures > 0 {
                            log::info!(
                                target: "desktop_host",
                                "desktop host recovered after {failures} failed attempt(s)"
                            );
                        }
                        failures = 0;
                        MONITOR_INTERVAL
                    }
                    Err(error) => {
                        failures = failures.saturating_add(1);
                        let delay = RETRY_DELAYS[(failures - 1).min(RETRY_DELAYS.len() - 1)];
                        log::warn!(
                            target: "desktop_host",
                            "desktop sync attempt {failures} failed; retrying in {} ms: {error}",
                            delay.as_millis()
                        );
                        signal.sync_requested.store(true, Ordering::Release);
                        delay
                    }
                }
            } else {
                MONITOR_INTERVAL
            };

            let Ok(guard) = signal.wake_lock.lock() else {
                log::error!(target: "desktop_host", "supervisor wake lock is poisoned");
                break;
            };
            if signal.shutting_down.load(Ordering::Acquire) {
                break;
            }
            let _ = signal.wake.wait_timeout(guard, wait_duration);
        }
        log::info!(target: "desktop_host", "desktop supervisor stopped");
    }

    fn log_host_exit(status: ExitStatus) {
        if status.success() {
            log::warn!(target: "desktop_host", "desktop host exited unexpectedly: {status}");
        } else {
            log::error!(target: "desktop_host", "desktop host crashed or failed: {status}");
        }
    }

    fn send_sync(
        controller: &mut ControllerState,
        executable: &Path,
        app: &AppHandle,
        command: &HostCommand,
    ) -> Result<(), SyncFailure> {
        let needs_start = match controller.running.as_mut() {
            Some(running) => running
                .child
                .try_wait()
                .map_err(|error| {
                    SyncFailure::Transport(format!("无法检查 Desktop Host 状态：{error}"))
                })?
                .is_some(),
            None => true,
        };
        if needs_start {
            stop_running(controller.running.take());
            controller.running = Some(spawn_host(executable, app).map_err(SyncFailure::Transport)?);
        }
        let running = controller
            .running
            .as_mut()
            .ok_or_else(|| SyncFailure::Transport("Desktop Host 没有启动".to_string()))?;
        send_command(&mut running.stdin, command).map_err(SyncFailure::Transport)?;
        let expected_revision = match command {
            HostCommand::Sync { revision, .. } => *revision,
            _ => return Ok(()),
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(SyncFailure::Transport(format!(
                    "Desktop Host 没有确认 revision {expected_revision}"
                )));
            }
            match running.events.recv_timeout(remaining) {
                Ok(Ok(HostEvent::Synced { revision, .. })) if revision == expected_revision => {
                    return Ok(());
                }
                Ok(Ok(HostEvent::Error { message })) => return Err(SyncFailure::Host(message)),
                Ok(Ok(_)) => continue,
                Ok(Err(error)) => return Err(SyncFailure::Transport(error)),
                Err(error) => {
                    return Err(SyncFailure::Transport(format!(
                        "等待 Desktop Host 同步确认失败：{error}"
                    )));
                }
            }
        }
    }

    fn spawn_host(executable: &Path, app: &AppHandle) -> Result<RunningHost, String> {
        log::info!(
            target: "desktop_host",
            "starting host executable={}",
            executable.display()
        );
        let mut child = Command::new(executable)
            .arg("--ipc-stdio")
            .env("DCREEL_LOG_DIR", crate::diagnostics::log_directory())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("无法启动 {}：{error}", executable.display()))?;
        log::info!(target: "desktop_host", "host process spawned pid={}", child.id());
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Desktop Host 没有提供 stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Desktop Host 没有提供 stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Desktop Host 没有提供 stderr".to_string())?;
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        log::warn!(target: "desktop_host_stderr", "{line}");
                    }
                    Ok(_) => {}
                    Err(error) => {
                        log::warn!(target: "desktop_host_stderr", "stderr relay stopped: {error}");
                        break;
                    }
                }
            }
        });
        let (sender, events) = mpsc::channel();
        let (request_sender, request_receiver) = mpsc::channel::<NativeRequest>();
        let request_app = app.clone();
        thread::spawn(move || {
            while let Ok(request) = request_receiver.recv() {
                let Some(store) = request_app.try_state::<AppStore>() else {
                    let _ = request_app.emit("creel://notification", "DCreel 状态尚未就绪");
                    continue;
                };
                let NativeRequest::UserAction(action) = request;
                let result = apply_user_action(&request_app, store, action);
                match result {
                    Ok(Some(message)) => {
                        let _ = request_app.emit("creel://notification", message);
                    }
                    Ok(None) => {}
                    Err(message) => {
                        let _ = request_app.emit("creel://notification", message);
                    }
                }
            }
        });
        let callback_app = app.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let event = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str::<HostEvent>(&line).map_err(|error| error.to_string())
                });
                if let Ok(HostEvent::Notification { message }) = &event {
                    let _ = callback_app.emit("creel://notification", message.clone());
                    desktop_notifications::show_message(
                        &callback_app,
                        "DCreel 无法完成操作",
                        message.clone(),
                    );
                    continue;
                }
                if let Ok(HostEvent::DesktopVisibilityChanged { visible }) = &event {
                    if let Some(store) = callback_app.try_state::<AppStore>() {
                        store.set_desktop_visible(*visible);
                        let _ = callback_app.emit("creel://state-changed", ());
                    }
                    if let Some(host) = callback_app.try_state::<DesktopHostController>() {
                        host.request_sync();
                    }
                    continue;
                }
                if let Ok(HostEvent::GeometryChanged {
                    id,
                    x,
                    y,
                    width,
                    height,
                    display_anchor,
                    placement,
                }) = &event
                {
                    if let Err(error) = persist_host_geometry(
                        &callback_app,
                        id,
                        *x,
                        *y,
                        *width,
                        *height,
                        display_anchor.clone(),
                        placement.clone(),
                    ) {
                        let _ = callback_app.emit(
                            "creel://notification",
                            format!("桌面盒子布局保存失败：{error}"),
                        );
                    }
                    continue;
                }
                if let Ok(HostEvent::ImportFiles { fence_id, paths }) = &event {
                    let enqueue = callback_app
                        .try_state::<FileTransferManager>()
                        .ok_or_else(|| "文件移动服务尚未就绪".to_string())
                        .and_then(|transfers| {
                            let store = callback_app.state::<AppStore>();
                            let title = store
                                .lock()
                                .map_err(|error| error.to_string())?
                                .fences
                                .iter()
                                .find(|fence| fence.id == *fence_id)
                                .map(|fence| fence.title.clone())
                                .ok_or_else(|| format!("没有找到盒子：{fence_id}"))?;
                            transfers.enqueue(fence_id.clone(), title, paths.clone())
                        });
                    if let Err(error) = enqueue {
                        let message = format!("无法开始文件移动：{error}");
                        let _ = callback_app.emit("creel://notification", &message);
                        desktop_notifications::show_message(
                            &callback_app,
                            "DCreel 文件移动",
                            message,
                        );
                    }
                    continue;
                }
                if let Ok(HostEvent::UserAction { action }) = &event {
                    if request_sender
                        .send(NativeRequest::UserAction(action.clone()))
                        .is_err()
                    {
                        let _ =
                            callback_app.emit("creel://notification", "桌面盒子的操作服务已停止");
                    }
                    continue;
                }
                if sender.send(event).is_err() {
                    return;
                }
            }
        });
        send_command(
            &mut stdin,
            &HostCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
        )?;
        match events.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(HostEvent::Ready { protocol_version }))
                if protocol_version == PROTOCOL_VERSION =>
            {
                log::info!(
                    target: "desktop_host",
                    "host handshake completed protocol={protocol_version}"
                );
            }
            Ok(Ok(HostEvent::Error { message })) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(message);
            }
            Ok(Ok(event)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Desktop Host 握手事件无效：{event:?}"));
            }
            Ok(Err(error)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 Desktop Host 握手失败：{error}"));
            }
        }
        Ok(RunningHost {
            child,
            stdin,
            events,
        })
    }

    fn apply_user_action(
        app: &AppHandle,
        store: tauri::State<'_, AppStore>,
        action: HostUserAction,
    ) -> Result<Option<String>, String> {
        if matches!(action, HostUserAction::QuitApplication) {
            crate::quit_application(app);
            return Ok(None);
        }
        if matches!(action, HostUserAction::CreateStorageBox) {
            crate::request_storage_box_ui(app);
            return Ok(None);
        }
        if matches!(action, HostUserAction::CreateMappedBox) {
            crate::request_mapped_fence_ui(app);
            return Ok(None);
        }

        let id = match &action {
            HostUserAction::RenameFence { id, .. }
            | HostUserAction::ToggleFenceCollapsed { id }
            | HostUserAction::ToggleFenceLocked { id }
            | HostUserAction::SetFenceColor { id, .. }
            | HostUserAction::ResetFenceSize { id }
            | HostUserAction::RemoveFence { id } => id,
            HostUserAction::CreateStorageBox
            | HostUserAction::CreateMappedBox
            | HostUserAction::QuitApplication => {
                unreachable!()
            }
        };
        let fence = store
            .lock()
            .map_err(|error| error.to_string())?
            .fences
            .iter()
            .find(|fence| fence.id == *id)
            .cloned()
            .ok_or_else(|| format!("没有找到盒子：{id}"))?;

        match action {
            HostUserAction::RenameFence { title, .. } => {
                let view = crate::commands::update_fence_inner(
                    &fence.id,
                    crate::models::FencePatch {
                        title: Some(title),
                        ..Default::default()
                    },
                    app,
                    &store,
                )?;
                Ok(Some(format!("已重命名为「{}」", view.config.title)))
            }
            HostUserAction::ToggleFenceCollapsed { .. } => {
                let collapsed = !fence.collapsed;
                let view = crate::commands::update_fence_inner(
                    &fence.id,
                    crate::models::FencePatch {
                        collapsed: Some(collapsed),
                        ..Default::default()
                    },
                    app,
                    &store,
                )?;
                Ok(Some(if collapsed {
                    format!("已收起「{}」", view.config.title)
                } else {
                    format!("已展开「{}」", view.config.title)
                }))
            }
            HostUserAction::ToggleFenceLocked { .. } => {
                let locked = !fence.locked;
                let view = crate::commands::update_fence_inner(
                    &fence.id,
                    crate::models::FencePatch {
                        locked: Some(locked),
                        ..Default::default()
                    },
                    app,
                    &store,
                )?;
                Ok(Some(if locked {
                    format!("已锁定「{}」的位置", view.config.title)
                } else {
                    format!("已解锁「{}」的位置", view.config.title)
                }))
            }
            HostUserAction::SetFenceColor { color, .. } => {
                crate::commands::update_fence_inner(
                    &fence.id,
                    crate::models::FencePatch {
                        color: Some(color),
                        ..Default::default()
                    },
                    app,
                    &store,
                )?;
                Ok(Some("已更新盒子颜色".into()))
            }
            HostUserAction::ResetFenceSize { .. } => {
                let view = crate::commands::reset_fence_size_inner(&fence.id, app, &store)?;
                Ok(Some(format!(
                    "已把「{}」恢复为默认大小 {} × {}",
                    view.config.title, view.config.width as i32, view.config.height as i32
                )))
            }
            HostUserAction::RemoveFence { .. } => {
                let title = fence.title;
                crate::commands::remove_fence(fence.id, app.clone(), store)?;
                Ok(Some(format!(
                    "已移除「{title}」；文件夹和其中的内容没有删除"
                )))
            }
            HostUserAction::CreateStorageBox
            | HostUserAction::CreateMappedBox
            | HostUserAction::QuitApplication => {
                unreachable!()
            }
        }
    }

    // HostEvent::GeometryChanged carries each persisted geometry field
    // independently; this boundary intentionally mirrors the IPC payload.
    #[allow(clippy::too_many_arguments)]
    fn persist_host_geometry(
        app: &AppHandle,
        id: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        display_anchor: Option<creel_ipc::DisplayAnchor>,
        placement: Option<creel_ipc::FencePlacement>,
    ) -> Result<(), String> {
        let Some(store) = app.try_state::<AppStore>() else {
            return Err("DCreel 状态尚未就绪".into());
        };
        let mut state = store.lock().map_err(|error| error.to_string())?;
        let Some(index) = state.fences.iter().position(|fence| fence.id == id) else {
            // 盒子可能刚被主线程删除；迟到的几何事件可以安全忽略。
            return Ok(());
        };
        let previous = state.fences[index].clone();
        let geometry_changed =
            crate::store::apply_fence_geometry(&mut state.fences[index], x, y, width, height);
        let display_changed = state.fences[index].display_anchor != display_anchor;
        let placement_changed = state.fences[index].placement != placement;
        state.fences[index].display_anchor = display_anchor;
        state.fences[index].placement = placement;
        if !geometry_changed && !display_changed && !placement_changed {
            return Ok(());
        }
        if let Err(error) = store.save(&state) {
            state.fences[index] = previous;
            return Err(error.to_string());
        }
        drop(state);
        let _ = app.emit("creel://state-changed", ());
        Ok(())
    }

    fn send_command(stdin: &mut ChildStdin, command: &HostCommand) -> Result<(), String> {
        let line = message_line(command).map_err(|error| error.to_string())?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("无法向 Desktop Host 写入 IPC：{error}"))
    }

    fn stop_running(running: Option<RunningHost>) {
        let Some(mut running) = running else {
            return;
        };
        log::info!(target: "desktop_host", "stopping host pid={}", running.child.id());
        let _ = send_command(&mut running.stdin, &HostCommand::Shutdown);
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match running.child.try_wait() {
                Ok(Some(status)) => {
                    log::info!(target: "desktop_host", "host stopped: {status}");
                    return;
                }
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                _ => break,
            }
        }
        let _ = running.child.kill();
        match running.child.wait() {
            Ok(status) => log::warn!(target: "desktop_host", "host was force-stopped: {status}"),
            Err(error) => log::warn!(target: "desktop_host", "failed waiting for host: {error}"),
        }
    }

    fn find_executable(app: &AppHandle) -> Result<PathBuf, String> {
        let mut candidates = Vec::new();
        if let Ok(executable) = std::env::current_exe()
            && let Some(directory) = executable.parent()
        {
            candidates.push(directory.join("creel-desktop-host.exe"));
        }
        if let Ok(directory) = app.path().resource_dir() {
            candidates.push(directory.join("creel-desktop-host.exe"));
        }
        for candidate in candidates {
            log::debug!(
                target: "desktop_host",
                "checking host executable candidate={}",
                candidate.display()
            );
            if candidate.is_file() {
                log::info!(
                    target: "desktop_host",
                    "host executable found at {}",
                    candidate.display()
                );
                return Ok(candidate);
            }
        }
        Err("已启用桌面宿主，但没有找到 creel-desktop-host.exe".into())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub struct DesktopHostController;

    impl DesktopHostController {
        pub fn new(_app: &AppHandle) -> Self {
            Self
        }

        pub fn start_supervisor(&self) -> Result<(), String> {
            Ok(())
        }

        pub fn request_sync(&self) {}

        pub fn is_active(&self) -> bool {
            false
        }

        pub fn sync(&self, _state: &PersistedState, _visible: bool) -> Result<(), String> {
            Ok(())
        }

        pub fn shutdown(&self) {}

        pub fn refresh_fence(&self, _id: &str) -> Result<(), String> {
            Ok(())
        }

        pub fn set_hotkey_capture_active(&self, _active: bool) -> Result<(), String> {
            Ok(())
        }
    }
}

pub use platform::DesktopHostController;
