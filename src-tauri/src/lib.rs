mod commands;
mod desktop_context_menu;
mod desktop_host;
mod desktop_notifications;
mod desktop_windows;
mod diagnostics;
mod directory_watchers;
mod file_transfers;
mod models;
mod store;

use creel_ipc::{ExternalCommand, parse_external_commands};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use store::AppStore;
use tauri::{
    Emitter, Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::MacosLauncher;

const ARG_UNREGISTER_SHELL: &str = "--unregister-shell-integration";
static BACKEND_READY: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct PendingNavigation(Mutex<Option<String>>);

struct StartupController {
    completed: AtomicBool,
    show_main_when_ready: AtomicBool,
}

impl StartupController {
    fn new(show_main_when_ready: bool) -> Self {
        Self {
            completed: AtomicBool::new(false),
            show_main_when_ready: AtomicBool::new(show_main_when_ready),
        }
    }
}

#[tauri::command]
fn take_pending_navigation(state: tauri::State<'_, PendingNavigation>) -> Option<String> {
    state.0.lock().ok()?.take()
}

#[tauri::command]
fn backend_ready() -> bool {
    BACKEND_READY.load(Ordering::Acquire)
}

fn shutdown_desktop_integration(app: &tauri::AppHandle) {
    if let Some(host) = app.try_state::<desktop_host::DesktopHostController>() {
        host.shutdown();
    }
}

pub(crate) fn quit_application(app: &tauri::AppHandle) {
    if app
        .try_state::<file_transfers::FileTransferManager>()
        .is_some_and(|transfers| transfers.is_active())
    {
        desktop_notifications::show_transfer_progress(app);
        desktop_notifications::show_message(
            app,
            "文件仍在移动",
            "请等待文件移动完成，或在进度窗口中安全取消后再退出 DCreel。",
        );
        log::warn!(target: "shutdown", "explicit quit deferred while a file transfer is active");
        return;
    }
    log::info!(target: "shutdown", "explicit quit command received");
    shutdown_desktop_integration(app);
    app.exit(0);
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    quit_application(&app);
}

#[tauri::command]
fn show_update_notification(version: String, app: tauri::AppHandle) -> Result<(), String> {
    let version: String = version.trim().chars().take(64).collect();
    if version.is_empty() {
        return Err("更新版本号为空".into());
    }
    let ignored = app
        .state::<AppStore>()
        .lock()
        .map_err(|error| error.to_string())?
        .preferences
        .ignored_update_version
        .as_deref()
        == Some(version.as_str());
    if !ignored {
        desktop_notifications::show_update(&app, version);
    }
    Ok(())
}

#[tauri::command]
fn ignore_update_version(version: String, app: tauri::AppHandle) -> Result<(), String> {
    let version: String = version.trim().chars().take(64).collect();
    if version.is_empty() {
        return Err("更新版本号为空".into());
    }
    let store = app.state::<AppStore>();
    {
        let mut state = store.lock().map_err(|error| error.to_string())?;
        state.preferences.ignored_update_version = Some(version.clone());
        store.save(&state).map_err(|error| error.to_string())?;
    }
    let _ = app.emit("creel://state-changed", ());
    log::info!(target: "updater", "desktop notification ignored version={version}");
    Ok(())
}

#[tauri::command]
fn show_update_details(app: tauri::AppHandle, notification_id: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("desktop-notification")
        && !desktop_notifications::dismiss_notification(
            &window,
            Some(&notification_id),
            "update-details",
        )?
    {
        return Ok(());
    }
    let _ = app.emit_to("main", "creel://navigate", "about");
    request_main_window(&app);
    Ok(())
}

#[tauri::command]
fn dismiss_auxiliary_window(
    window: tauri::WebviewWindow,
    notification_id: Option<String>,
) -> Result<(), String> {
    match window.label() {
        "transfer-progress" => window
            .state::<file_transfers::FileTransferManager>()
            .dismiss_progress_window(&window),
        "desktop-notification" => {
            let id = notification_id.ok_or("缺少通知编号")?;
            desktop_notifications::dismiss_notification(&window, Some(&id), "dismiss-command")
                .map(|_| ())
        }
        _ => desktop_notifications::hide_auxiliary_window(&window, "dismiss-command"),
    }
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        match window.label() {
            "main" | "new-box" => {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    log::error!(target: "window", "failed to hide window label={}: {error}", window.label());
                }
            }
            "desktop-notification" | "transfer-progress" => {
                api.prevent_close();
                if let Some(webview) = window.get_webview_window(window.label()) {
                    if window.label() == "desktop-notification" {
                        let _ = desktop_notifications::dismiss_notification(
                            &webview,
                            None,
                            "native-close",
                        );
                    } else {
                        let _ = dismiss_auxiliary_window(webview, None);
                    }
                }
            }
            _ => {}
        }
    }
    if matches!(window.label(), "desktop-notification" | "transfer-progress")
        && matches!(
            event,
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }
        )
    {
        let app = window.app_handle().clone();
        let label = window.label().to_string();
        // Defer until the native resize/DPI callback has finished.
        tauri::async_runtime::spawn(async move {
            let ui_app = app.clone();
            let _ = app.run_on_main_thread(move || {
                desktop_notifications::relayout_windows(&ui_app, &label)
            });
        });
    }
}

fn request_desktop_sync(app: &tauri::AppHandle) {
    if let Some(host) = app.try_state::<desktop_host::DesktopHostController>() {
        host.request_sync();
    }
}

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示 DCreel", true, None::<&str>)?;
    let new_fence = MenuItem::with_id(app, "new-fence", "新建收纳盒", true, None::<&str>)?;
    let new_mapped =
        MenuItem::with_id(app, "new-mapped-fence", "新建映射盒子", true, None::<&str>)?;
    let toggle_fences = MenuItem::with_id(
        app,
        "toggle-fences",
        "显示/隐藏桌面盒子",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show, &new_fence, &new_mapped, &toggle_fences, &quit],
    )?;

    let mut tray = TrayIconBuilder::with_id("creel-tray")
        .tooltip("DCreel · 把桌面轻轻收好")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => request_main_window(app),
            "toggle-fences" => {
                let _ = desktop_windows::toggle_visibility(app);
            }
            "new-fence" => request_storage_box_ui(app),
            "new-mapped-fence" => request_mapped_fence_ui(app),
            "quit" => {
                quit_application(app);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                let app = tray.app_handle();
                request_main_window(app);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    let tray = tray.build(app)?;
    let show_tray_icon = app
        .state::<AppStore>()
        .lock()
        .map(|state| state.preferences.show_tray_icon)
        .unwrap_or(true);
    tray.set_visible(show_tray_icon)?;
    Ok(())
}

fn show_main(app: &tauri::AppHandle) {
    request_desktop_sync(app);
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.show() {
            log::error!(target: "window", "failed to show main window: {error}");
        }
        if let Err(error) = window.unminimize() {
            log::warn!(target: "window", "failed to unminimize main window: {error}");
        }
        if let Err(error) = window.set_focus() {
            log::warn!(target: "window", "failed to focus main window: {error}");
        }
    } else {
        log::error!(target: "window", "main window is unavailable");
    }
}

fn request_main_window(app: &tauri::AppHandle) {
    let Some(startup) = app.try_state::<StartupController>() else {
        show_main(app);
        return;
    };
    startup.show_main_when_ready.store(true, Ordering::Release);
    if startup.completed.load(Ordering::Acquire) {
        show_main(app);
    }
}

fn finish_startup(app: &tauri::AppHandle, source: &str) {
    let Some(startup) = app.try_state::<StartupController>() else {
        log::error!(target: "startup", "startup controller is unavailable");
        return;
    };
    if startup.completed.swap(true, Ordering::AcqRel) {
        return;
    }
    log::info!(target: "startup", "startup completed source={source}");
    if let Some(splash) = app.get_webview_window("splash")
        && let Err(error) = splash.close()
    {
        log::warn!(target: "window", "failed to close splash window: {error}");
    }
    if startup.show_main_when_ready.load(Ordering::Acquire) {
        show_main(app);
    }
}

#[tauri::command]
fn complete_startup(app: tauri::AppHandle) {
    finish_startup(&app, "frontend");
}

fn request_new_box_ui(app: &tauri::AppHandle, route: &str) {
    if let Some(pending) = app.try_state::<PendingNavigation>()
        && let Ok(mut pending) = pending.0.lock()
    {
        *pending = Some(route.into());
    }
    let _ = app.emit_to("new-box", "creel://navigate", route);
    if let Some(window) = app.get_webview_window("new-box") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn request_storage_box_ui(app: &tauri::AppHandle) {
    request_new_box_ui(app, "new-storage-box");
}

pub(crate) fn request_mapped_fence_ui(app: &tauri::AppHandle) {
    request_new_box_ui(app, "new-mapped-box");
}

fn handle_external_args(app: &tauri::AppHandle, args: &[String]) {
    log::info!(target: "ipc", "handling external command arguments count={}", args.len());
    let parsed = parse_external_commands(args);
    let mut should_show = parsed.is_empty();
    for command in parsed {
        match command {
            ExternalCommand::Show => should_show = true,
            ExternalCommand::Silent => {}
            ExternalCommand::NewStorageBox => {
                request_storage_box_ui(app);
            }
            ExternalCommand::NewMappedBox => {
                request_mapped_fence_ui(app);
            }
            ExternalCommand::ToggleFences => {
                if let Err(error) = desktop_windows::toggle_visibility(app) {
                    let _ = app.emit("creel://notification", error);
                }
            }
            ExternalCommand::Quit => {
                quit_application(app);
                return;
            }
            ExternalCommand::MapFolder(path) => {
                let store = app.state::<AppStore>();
                match commands::map_folder_inner(&path, app, &store) {
                    Ok(view) => {
                        let _ = app.emit(
                            "creel://notification",
                            format!("已把「{}」映射为桌面盒子", view.config.title),
                        );
                    }
                    Err(error) => {
                        let _ = app.emit("creel://notification", error);
                    }
                }
            }
        }
    }
    if should_show {
        request_main_window(app);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    BACKEND_READY.store(false, Ordering::Release);
    let startup_args: Vec<String> = std::env::args().collect();
    diagnostics::initialize(&startup_args);
    let unregister_shell = startup_args
        .iter()
        .any(|argument| argument == ARG_UNREGISTER_SHELL);
    if unregister_shell {
        if let Err(error) = desktop_context_menu::unregister() {
            eprintln!("DCreel could not remove Explorer integration: {error}");
        }
        return;
    }

    let initial_commands = parse_external_commands(&startup_args);
    let show_main_when_ready = initial_commands.is_empty()
        || initial_commands
            .iter()
            .any(|command| matches!(command, ExternalCommand::Show));
    let initial_args = startup_args.clone();

    let builder = tauri::Builder::default()
        .manage(desktop_notifications::NotificationManager::default())
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started)
                && let Some(window) = webview.get_webview_window(webview.label())
            {
                desktop_notifications::notification_page_loading(&window);
            }
        })
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            log::info!(target: "ipc", "secondary instance received");
            handle_external_args(app, &args);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--silent"]),
        ))
        .setup(move |app| {
            log::info!(target: "startup", "Tauri setup started");
            let store = AppStore::load(app.handle())?;
            let context_menu_enabled = store
                .lock()
                .map(|state| state.preferences.desktop_context_menu)
                .unwrap_or(true);
            app.manage(store);
            app.manage(PendingNavigation::default());
            app.manage(StartupController::new(show_main_when_ready));
            app.manage(file_transfers::FileTransferManager::new(app.handle())?);
            let desktop_host = desktop_host::DesktopHostController::new(app.handle());
            app.manage(desktop_host);
            app.manage(directory_watchers::DirectoryWatchers::default());
            if let Err(error) = install_tray(app) {
                log::error!(target: "tray", "tray initialization failed: {error}");
            }
            if let Some(host) = app.try_state::<desktop_host::DesktopHostController>()
                && let Err(error) = host.start_supervisor()
            {
                log::error!(target: "desktop_host", "desktop supervisor failed to start: {error}");
            }
            if let Err(error) =
                desktop_context_menu::set_enabled(app.handle(), context_menu_enabled)
            {
                log::error!(target: "shell_integration", "context menu setup failed: {error}");
            }
            BACKEND_READY.store(true, Ordering::Release);
            log::info!(target: "startup", "backend state is ready");
            handle_external_args(app.handle(), &initial_args);

            let timeout_app = app.handle().clone();
            if let Err(error) =
                thread::Builder::new()
                    .name("startup-timeout".into())
                    .spawn(move || {
                        thread::sleep(Duration::from_secs(15));
                        if timeout_app
                            .try_state::<StartupController>()
                            .is_some_and(|startup| !startup.completed.load(Ordering::Acquire))
                        {
                            log::error!(
                                target: "startup",
                                "frontend did not complete startup within 15 seconds"
                            );
                            finish_startup(&timeout_app, "native-timeout");
                        }
                    })
            {
                log::error!(target: "startup", "failed to create startup timeout: {error}");
            }
            log::info!(target: "startup", "Tauri setup completed");
            Ok(())
        })
        .on_window_event(handle_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::load_dashboard,
            commands::create_storage_box,
            commands::create_mapped_box,
            commands::update_fence,
            commands::remove_fence,
            commands::update_preferences,
            commands::set_desktop_visibility,
            commands::open_path,
            commands::open_project_repository,
            commands::set_hotkey_capture_active,
            diagnostics::write_frontend_log,
            diagnostics::open_log_directory,
            complete_startup,
            backend_ready,
            take_pending_navigation,
            quit_app,
            show_update_notification,
            ignore_update_version,
            show_update_details,
            dismiss_auxiliary_window,
            desktop_notifications::current_desktop_notification,
            desktop_notifications::subscribe_desktop_notifications,
            desktop_notifications::present_desktop_notification,
            file_transfers::cancel_file_transfer,
            file_transfers::current_file_transfer,
        ]);

    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(error) => {
            log::error!(target: "startup", "Tauri application build failed: {error}");
            creel_logging::flush();
            eprintln!("DCreel 启动失败，详情请查看日志：{error}");
            return;
        }
    };

    app.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            log::info!(target: "shutdown", "application exit requested");
            shutdown_desktop_integration(app);
            creel_logging::flush();
        }
    });
}
