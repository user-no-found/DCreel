//! Real WebView2 + HWND regression test. No desktop host, shell registration,
//! tray, autostart or updater is started. Shortcut actions use isolated temporary fixtures.
//! Run after `npm run build` on Windows:
//! cargo test --manifest-path src-tauri/Cargo.toml --features tauri/custom-protocol \
//!   --lib native_progress_window_lifecycle -- --ignored --nocapture --test-threads=1

use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};
use tauri::WebviewWindowBuilder;

#[path = "file_transfer_window_test/notifications.rs"]
mod notifications;
#[path = "file_transfer_window_test/shortcut_notifications.rs"]
mod shortcut_notifications;
#[path = "file_transfer_window_test/support.rs"]
mod support;
use support::*;

struct FixtureState {
    fail_dashboard: AtomicBool,
}

struct ProbeLogger;
impl log::Log for ProbeLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            println!("{} {} {}", record.level(), record.target(), record.args());
        }
    }
    fn flush(&self) {}
}
static PROBE_LOGGER: ProbeLogger = ProbeLogger;

#[tauri::command]
fn backend_ready() -> bool {
    true
}

#[tauri::command]
fn load_dashboard(
    fixture: tauri::State<'_, FixtureState>,
) -> Result<crate::models::Dashboard, String> {
    if fixture.fail_dashboard.load(Ordering::Acquire) {
        return Err("测试配置读取失败（不读写真实配置）".into());
    }
    Ok(crate::models::Dashboard {
        fences: Vec::new(),
        preferences: Default::default(),
        desktop_visible: true,
    })
}

fn begin(app: &AppHandle, state: &Arc<Mutex<TransferState>>, id: &str) {
    let mut snapshot = TransferSnapshot::queued(id.into(), "窗口回归测试（不移动文件）".into());
    snapshot.phase = "moving";
    snapshot.total_bytes = 12_000_000_000;
    snapshot.total_items = 27_932;
    let mut state = state.lock().unwrap();
    state.snapshot = Some(snapshot.clone());
    state.dismissed_id = None;
    state.active_cancel = Some((id.into(), Arc::new(AtomicBool::new(false))));
    emit_snapshot(app, &snapshot);
}

fn finish(app: &AppHandle, state: &Arc<Mutex<TransferState>>, phase: &'static str) {
    let mut snapshot = state.lock().unwrap().snapshot.clone().unwrap();
    snapshot.phase = phase;
    snapshot.can_cancel = false;
    snapshot.completed_bytes = snapshot.total_bytes;
    snapshot.completed_items = snapshot.total_items;
    let id = snapshot.id.clone();
    finish_with_snapshot(app, state, &id, snapshot);
}

fn exercise(app: &AppHandle, state: &Arc<Mutex<TransferState>>) {
    let notification = app.get_webview_window("desktop-notification").unwrap();
    wait_for(
        "notification emitted before UI readiness is restored and shown",
        Duration::from_secs(15),
        || visible(&notification),
    );
    assert!(
        javascript(
            &notification,
            "document.body.textContent.includes('早于页面就绪的通知')"
        ) == true
    );
    javascript(
        &notification,
        "document.querySelector('.desktop-notification-close').click()",
    );
    wait_for("early notification closes", Duration::from_secs(2), || {
        !visible(&notification)
    });
    println!("PASS: early notification retained until render acknowledgement; no blank popup");
    let window = app.get_webview_window(PROGRESS_WINDOW).unwrap();
    assert!(!visible(&window));
    begin(app, state, "manual-active");
    wait_for(
        "actual progress React UI ready",
        Duration::from_secs(15),
        || {
            javascript(
                &window,
                "!!document.querySelector('.transfer-window header button')",
            ) == true
        },
    );

    // These HWND assertions reproduce the old native-show / runtime-hide mismatch.
    show(app);
    assert!(visible(&window));
    assert!(
        javascript(
            &window,
            "(() => { document.querySelector('.transfer-window header button').click(); return true; })()"
        ) == true
    );
    wait_for(
        "header button hides HWND through real IPC",
        Duration::from_secs(2),
        || !visible(&window),
    );
    assert!(
        state.lock().unwrap().active_cancel.is_some(),
        "hiding must not cancel the transfer"
    );
    schedule_progress_window(app, state, "manual-active", ProgressWindowAction::Show);
    thread::sleep(PROGRESS_WINDOW_DELAY + Duration::from_millis(200));
    assert!(
        !visible(&window),
        "delayed show reopened a manually hidden task"
    );
    println!(
        "PASS: header button -> IPC -> native hide; transfer remains active; no delayed reopen"
    );

    begin(app, state, "manual-completed");
    show(app);
    finish(app, state, "completed");
    wait_for(
        "completed footer close button",
        Duration::from_secs(2),
        || {
            javascript(
                &window,
                "!!document.querySelector('.phase-completed footer button')",
            ) == true
        },
    );
    javascript(
        &window,
        "document.querySelector('.phase-completed footer button').click()",
    );
    wait_for("footer button hides HWND", Duration::from_secs(2), || {
        !visible(&window)
    });
    println!("PASS: completed footer close button -> IPC -> native hide");

    for phase in ["completed", "cancelled", "failed"] {
        begin(app, state, phase);
        show(app);
        let started = Instant::now();
        finish(app, state, phase);
        thread::sleep(Duration::from_millis(2_500));
        assert!(visible(&window), "{phase}: dismissed before 3 seconds");
        wait_for("native 3-second dismissal", Duration::from_secs(2), || {
            !visible(&window)
        });
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_secs(3));
        assert!(elapsed < Duration::from_millis(4_500));
        println!("PASS: {phase} auto-dismissed native HWND after {elapsed:?}");
    }

    begin(app, state, "old-completion");
    show(app);
    finish(app, state, "completed");
    begin(app, state, "next-active");
    thread::sleep(PROGRESS_DISMISS_DELAY + Duration::from_millis(200));
    assert!(
        visible(&window),
        "old completion timer hid the new active task"
    );
    println!("PASS: old task timeout cannot hide a new active transfer");

    notifications::exercise(app, &window);
    shortcut_notifications::exercise(app);

    let new_box = app.get_webview_window("new-box").unwrap();
    let ui_app = app.clone();
    on_ui(app, move || crate::request_storage_box_ui(&ui_app));
    wait_for(
        "new box configuration error has close/retry actions",
        Duration::from_secs(5),
        || {
            javascript(
                &new_box,
                "!!document.querySelector('.window-startup-error') && !!document.querySelector('.window-startup .modal-close')",
            ) == true
        },
    );
    javascript(
        &new_box,
        "document.querySelector('.window-startup .modal-close').click()",
    );
    wait_for("failed new-box close", Duration::from_secs(2), || {
        !visible(&new_box)
    });
    let ui_app = app.clone();
    on_ui(app, move || crate::request_storage_box_ui(&ui_app));
    app.state::<FixtureState>()
        .fail_dashboard
        .store(false, Ordering::Release);
    javascript(
        &new_box,
        "document.querySelector('.window-startup-actions .primary').click()",
    );
    wait_for(
        "retry recovers actual new-box form",
        Duration::from_secs(5),
        || javascript(&new_box, "!!document.querySelector('.folder-picker')") == true,
    );
    javascript(
        &new_box,
        "document.querySelector('.modal-card .modal-close').click()",
    );
    wait_for(
        "recovered new-box form closes",
        Duration::from_secs(2),
        || !visible(&new_box),
    );
    println!(
        "PASS: failed new-box loading remains closable; retry restores form without real configuration"
    );
}

#[test]
#[ignore = "requires interactive Windows/WebView2 and a built frontend; see module instructions"]
fn native_progress_window_lifecycle() {
    let _ = log::set_logger(&PROBE_LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    let probe_directory =
        std::env::temp_dir().join(format!("dcreel-window-test-{}", Uuid::new_v4()));
    let state = Arc::new(Mutex::new(TransferState::default()));
    let worker_state = Arc::clone(&state);
    let (sender, _receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let mut context = tauri::generate_context!();
    let config = context.config_mut();
    config.identifier = "com.dcreel.window-test".into();
    config.build.dev_url = None;
    let mut configs = std::mem::take(&mut config.app.windows);
    configs.retain(|window| {
        matches!(
            window.label.as_str(),
            "desktop-notification" | PROGRESS_WINDOW | "new-box"
        )
    });
    // Build the notification last and send immediately, without waiting for its
    // JS subscriber. The previous event-only implementation lost this payload.
    configs.sort_by_key(|window| window.label == "desktop-notification");
    let app = tauri::Builder::default()
        .any_thread()
        .manage(FileTransferManager { sender, state })
        .manage(desktop_notifications::NotificationManager::default())
        .manage(FixtureState {
            fail_dashboard: AtomicBool::new(true),
        })
        .manage(crate::PendingNavigation(Mutex::new(Some(
            "new-storage-box".into(),
        ))))
        .on_window_event(crate::handle_window_event)
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started)
                && let Some(window) = webview.get_webview_window(webview.label())
            {
                desktop_notifications::notification_page_loading(&window);
            }
        })
        .invoke_handler(tauri::generate_handler![
            crate::dismiss_auxiliary_window,
            current_file_transfer,
            crate::diagnostics::write_frontend_log,
            desktop_notifications::current_desktop_notification,
            desktop_notifications::subscribe_desktop_notifications,
            desktop_notifications::present_desktop_notification,
            desktop_notifications::delete_notification_shortcut,
            backend_ready,
            load_dashboard,
            crate::take_pending_navigation,
        ])
        .setup(move |app| {
            for config in configs {
                let window = WebviewWindowBuilder::from_config(app, &config)?
                    .data_directory(probe_directory.join(&config.label))
                    .build()?;
                // JS exercises real click handlers/IPC, while physical mouse
                // input passes through these test popups to the user's apps.
                window.set_ignore_cursor_events(true)?;
            }
            desktop_notifications::show_message(
                app.handle(),
                "早于页面就绪的通知",
                "必须先渲染再显示",
            );
            let app = app.handle().clone();
            thread::spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| exercise(&app, &worker_state)));
                let _ = result_sender.send(result);
                app.exit(0);
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    if let Err(error) = result_receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    {
        std::panic::resume_unwind(error);
    }
}
