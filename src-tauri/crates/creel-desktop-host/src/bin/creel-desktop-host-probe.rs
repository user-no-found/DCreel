use creel_ipc::{
    GhostModeTrigger, HostCommand, HostEvent, HostFenceSnapshot, HostPreferencesSnapshot,
    PROTOCOL_VERSION, message_line,
};
use std::{
    ffi::c_void,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(windows)]
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            SystemServices::{MK_CONTROL, MK_LBUTTON},
        },
        UI::{
            Input::KeyboardAndMouse::{
                KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, SetFocus, VK_APPS, VK_ESCAPE, VK_F5, VK_RIGHT,
                keybd_event,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW,
                GetCursorPos, GetForegroundWindow, GetWindowRect, IsWindowVisible, MSG, PM_REMOVE,
                PeekMessageW, PostMessageW, RegisterClassW, SendMessageW, SetCursorPos,
                SetForegroundWindow, TranslateMessage, WM_CANCELMODE, WM_CONTEXTMENU, WM_KEYDOWN,
                WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSW, WS_CAPTION,
                WS_POPUP, WS_VISIBLE,
            },
        },
    },
    core::PCWSTR,
};

#[cfg(windows)]
static REPLAY_PROBE_KEY_DOWNS: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static REPLAY_PROBE_KEY_UPS: AtomicUsize = AtomicUsize::new(0);

#[cfg(windows)]
unsafe extern "system" fn replay_probe_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_KEYDOWN {
        REPLAY_PROBE_KEY_DOWNS.fetch_add(1, Ordering::SeqCst);
    } else if message == WM_KEYUP {
        REPLAY_PROBE_KEY_UPS.fetch_add(1, Ordering::SeqCst);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn send(stdin: &mut ChildStdin, command: &HostCommand) -> Result<(), String> {
    let line = message_line(command).map_err(|error| error.to_string())?;
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("无法向 Desktop Host 写入 IPC：{error}"))
}

fn receive(receiver: &Receiver<Result<HostEvent, String>>) -> Result<HostEvent, String> {
    let event = receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("等待 Desktop Host 事件超时：{error}"))??;
    Ok(event)
}

fn expect_ready(receiver: &Receiver<Result<HostEvent, String>>) -> Result<(), String> {
    match receive(receiver)? {
        HostEvent::Ready { protocol_version } if protocol_version == PROTOCOL_VERSION => Ok(()),
        HostEvent::Error { message } => Err(message),
        event => Err(format!("期望 Ready，实际收到 {event:?}")),
    }
}

fn expect_synced(
    receiver: &Receiver<Result<HostEvent, String>>,
    revision: u64,
    fence_count: usize,
) -> Result<(), String> {
    loop {
        match receive(receiver)? {
            HostEvent::Synced {
                revision: actual_revision,
                fence_count: actual_count,
            } if actual_revision == revision && actual_count == fence_count => return Ok(()),
            HostEvent::GeometryChanged { .. } => continue,
            HostEvent::Error { message } => return Err(message),
            event => {
                return Err(format!(
                    "期望 Synced({revision}, {fence_count})，实际收到 {event:?}"
                ));
            }
        }
    }
}

fn expect_geometry(
    receiver: &Receiver<Result<HostEvent, String>>,
    id: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    match receive(receiver)? {
        HostEvent::GeometryChanged {
            id: actual_id,
            x: actual_x,
            y: actual_y,
            width: actual_width,
            height: actual_height,
            ..
        } if actual_id == id
            && near(actual_x, x)
            && near(actual_y, y)
            && near(actual_width, width)
            && near(actual_height, height) =>
        {
            Ok(())
        }
        HostEvent::Error { message } => Err(message),
        event => Err(format!(
            "期望 GeometryChanged({id}, {x}, {y}, {width}, {height})，实际收到 {event:?}"
        )),
    }
}

fn near(left: f64, right: f64) -> bool {
    (left - right).abs() < 0.5
}

#[cfg(windows)]
fn probe_window(title: &str) -> Result<HWND, String> {
    let class_name: Vec<u16> = "Creel.DesktopHost.IpcWindow.v1"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let window_title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    unsafe { FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR(window_title.as_ptr())) }
        .map_err(|error| format!("没有找到 IPC 探针盒子窗口：{error}"))
}

#[cfg(windows)]
fn wait_for_window_visibility(window: HWND, expected: bool, action: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let visible = unsafe { IsWindowVisible(window).as_bool() };
        if visible == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{action}后盒子可见状态不正确：期望 {expected}，实际 {visible}"
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
fn send_keyboard_chord(keys: &[u8]) {
    for key in keys {
        unsafe { keybd_event(*key, 0, KEYBD_EVENT_FLAGS(0), 0) };
        thread::sleep(Duration::from_millis(20));
    }
    for key in keys.iter().rev() {
        unsafe { keybd_event(*key, 0, KEYEVENTF_KEYUP, 0) };
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
fn exercise_replayed_hotkey_members(keys: &[u8]) -> Result<(), String> {
    let class_name: Vec<u16> = "DCreel.HotkeyReplayProbe.v1"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let title: Vec<u16> = "DCreel 快捷键回放探针"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let module = unsafe { GetModuleHandleW(None) }
        .map_err(|error| format!("无法读取快捷键回放探针模块：{error}"))?;
    let instance = HINSTANCE(module.0);
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(replay_probe_window_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(format!(
            "无法注册快捷键回放探针窗口：{}",
            windows::core::Error::from_thread()
        ));
    }
    let previous_foreground = unsafe { GetForegroundWindow() };
    let window = unsafe {
        CreateWindowExW(
            Default::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_CAPTION | WS_VISIBLE,
            24,
            24,
            280,
            90,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|error| format!("无法创建快捷键回放探针窗口：{error}"))?;

    let result = (|| {
        if !unsafe { SetForegroundWindow(window).as_bool() } {
            return Err("无法激活快捷键回放探针窗口".into());
        }
        let _ = unsafe { SetFocus(Some(window)) };
        REPLAY_PROBE_KEY_DOWNS.store(0, Ordering::SeqCst);
        REPLAY_PROBE_KEY_UPS.store(0, Ordering::SeqCst);
        for key in keys {
            send_keyboard_chord(&[*key]);
        }

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, Some(window), 0, 0, PM_REMOVE).as_bool() } {
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            let key_downs = REPLAY_PROBE_KEY_DOWNS.load(Ordering::SeqCst);
            let key_ups = REPLAY_PROBE_KEY_UPS.load(Ordering::SeqCst);
            if key_downs == keys.len() && key_ups == keys.len() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "普通按键没有完整回放到前台窗口：期望 {0} 次按下和松开，实际 {key_downs} 次按下、{key_ups} 次松开",
                    keys.len()
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();

    let _ = unsafe { DestroyWindow(window) };
    if !previous_foreground.0.is_null() {
        let _ = unsafe { SetForegroundWindow(previous_foreground) };
    }
    result
}

#[cfg(windows)]
fn exercise_ghost_hotkey(window: HWND, keys: &[u8], label: &str) -> Result<(), String> {
    wait_for_window_visibility(window, true, "测试快捷键前")?;
    send_keyboard_chord(keys);
    wait_for_window_visibility(window, false, &format!("按下 {label}"))?;
    send_keyboard_chord(keys);
    wait_for_window_visibility(window, true, &format!("再次按下 {label}"))
}

#[cfg(windows)]
fn exercise_suspended_ghost_hotkey(window: HWND, keys: &[u8], label: &str) -> Result<(), String> {
    wait_for_window_visibility(window, true, "暂停快捷键测试前")?;
    send_keyboard_chord(keys);
    thread::sleep(Duration::from_millis(200));
    let visible = unsafe { IsWindowVisible(window).as_bool() };
    if visible {
        Ok(())
    } else {
        Err(format!("录入快捷键期间按下 {label} 仍然隐藏了盒子"))
    }
}

#[cfg(windows)]
fn exercise_window_geometry(
    receiver: &Receiver<Result<HostEvent, String>>,
    id: &str,
    title: &str,
    item_count: usize,
) -> Result<(), String> {
    let window = probe_window(title)?;
    let mut original_cursor = POINT::default();
    unsafe { GetCursorPos(&mut original_cursor) }
        .map_err(|error| format!("无法保存鼠标位置：{error}"))?;

    let result = (|| {
        let mut original_rect = RECT::default();
        unsafe { GetWindowRect(window, &mut original_rect) }
            .map_err(|error| format!("无法读取盒子初始矩形：{error}"))?;
        let start_x = original_rect.left + 100;
        let start_y = original_rect.top + 24;
        unsafe { SetCursorPos(start_x, start_y) }
            .map_err(|error| format!("无法定位移动探针鼠标：{error}"))?;
        unsafe {
            SendMessageW(window, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(LPARAM(0)));
            SetCursorPos(start_x + 37, start_y + 29)
                .map_err(|error| format!("无法移动探针鼠标：{error}"))?;
            SendMessageW(window, WM_MOUSEMOVE, Some(WPARAM(1)), Some(LPARAM(0)));
            SendMessageW(window, WM_LBUTTONUP, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        let width = f64::from(original_rect.right - original_rect.left);
        let height = f64::from(original_rect.bottom - original_rect.top);
        expect_geometry(
            receiver,
            id,
            f64::from(original_rect.left + 37),
            f64::from(original_rect.top + 29),
            width,
            height,
        )?;

        let mut moved_rect = RECT::default();
        unsafe { GetWindowRect(window, &mut moved_rect) }
            .map_err(|error| format!("无法读取移动后矩形：{error}"))?;
        let resize_x = moved_rect.right - 2;
        let resize_y = moved_rect.bottom - 2;
        unsafe { SetCursorPos(resize_x, resize_y) }
            .map_err(|error| format!("无法定位缩放探针鼠标：{error}"))?;
        unsafe {
            SendMessageW(window, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(LPARAM(0)));
            SetCursorPos(resize_x + 40, resize_y + 30)
                .map_err(|error| format!("无法缩放探针窗口：{error}"))?;
            SendMessageW(window, WM_MOUSEMOVE, Some(WPARAM(1)), Some(LPARAM(0)));
            SendMessageW(window, WM_LBUTTONUP, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        expect_geometry(
            receiver,
            id,
            f64::from(moved_rect.left),
            f64::from(moved_rect.top),
            f64::from(moved_rect.right - moved_rect.left + 40),
            f64::from(moved_rect.bottom - moved_rect.top + 30),
        )?;

        let mut resized_rect = RECT::default();
        unsafe { GetWindowRect(window, &mut resized_rect) }
            .map_err(|error| format!("无法读取缩放后的盒子矩形：{error}"))?;
        if item_count > 0 {
            exercise_item_selection(window, &resized_rect, item_count)?;
            exercise_keyboard_context_menu(window)?;
        }
        exercise_context_menu(
            window,
            resized_rect.left + 40,
            resized_rect.top + 24,
            "盒子",
        )?;
        if item_count > 0 {
            exercise_context_menu(
                window,
                resized_rect.left + 40,
                resized_rect.top + 70,
                "Explorer 项目",
            )?;
        }
        Ok(())
    })();

    let _ = unsafe { SetCursorPos(original_cursor.x, original_cursor.y) };
    result
}

#[cfg(windows)]
fn exercise_item_selection(window: HWND, rect: &RECT, item_count: usize) -> Result<(), String> {
    let click = |client_x: i32, client_y: i32, modifiers: usize| -> Result<(), String> {
        unsafe { SetCursorPos(rect.left + client_x, rect.top + client_y) }
            .map_err(|error| format!("无法定位选择探针鼠标：{error}"))?;
        let packed = u32::from(client_x as u16) | (u32::from(client_y as u16) << 16);
        unsafe {
            SendMessageW(
                window,
                WM_LBUTTONDOWN,
                Some(WPARAM(MK_LBUTTON.0 as usize | modifiers)),
                Some(LPARAM(packed as isize)),
            );
            SendMessageW(
                window,
                WM_LBUTTONUP,
                Some(WPARAM(modifiers)),
                Some(LPARAM(packed as isize)),
            );
        }
        Ok(())
    };
    click(40, 70, 0)?;
    if item_count > 1 {
        click(128, 70, MK_CONTROL.0 as usize)?;
    }
    unsafe {
        SendMessageW(
            window,
            WM_KEYDOWN,
            Some(WPARAM(usize::from(VK_F5.0))),
            Some(LPARAM(0)),
        );
        SendMessageW(
            window,
            WM_KEYDOWN,
            Some(WPARAM(usize::from(VK_RIGHT.0))),
            Some(LPARAM(0)),
        );
        SendMessageW(
            window,
            WM_KEYDOWN,
            Some(WPARAM(usize::from(VK_ESCAPE.0))),
            Some(LPARAM(0)),
        );
    }

    let start = POINT { x: 400, y: 80 };
    let end = POINT { x: 20, y: 170 };
    unsafe { SetCursorPos(rect.left + start.x, rect.top + start.y) }
        .map_err(|error| format!("无法定位框选起点：{error}"))?;
    let start_packed = u32::from(start.x as u16) | (u32::from(start.y as u16) << 16);
    let end_packed = u32::from(end.x as u16) | (u32::from(end.y as u16) << 16);
    unsafe {
        SendMessageW(
            window,
            WM_LBUTTONDOWN,
            Some(WPARAM(MK_LBUTTON.0 as usize)),
            Some(LPARAM(start_packed as isize)),
        );
        SetCursorPos(rect.left + end.x, rect.top + end.y)
            .map_err(|error| format!("无法移动框选探针鼠标：{error}"))?;
        SendMessageW(
            window,
            WM_MOUSEMOVE,
            Some(WPARAM(MK_LBUTTON.0 as usize)),
            Some(LPARAM(end_packed as isize)),
        );
        SendMessageW(
            window,
            WM_LBUTTONUP,
            Some(WPARAM(0)),
            Some(LPARAM(end_packed as isize)),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn exercise_keyboard_context_menu(window: HWND) -> Result<(), String> {
    let window_raw = window.0 as usize;
    let cancel_menu = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        unsafe {
            let _ = PostMessageW(
                Some(HWND(window_raw as *mut c_void)),
                WM_CANCELMODE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    });
    unsafe {
        SendMessageW(
            window,
            WM_KEYDOWN,
            Some(WPARAM(usize::from(VK_APPS.0))),
            Some(LPARAM(0)),
        );
    }
    cancel_menu
        .join()
        .map_err(|_| "键盘 Explorer 菜单取消探针线程异常".to_string())
}

#[cfg(windows)]
fn exercise_context_menu(window: HWND, x: i32, y: i32, label: &str) -> Result<(), String> {
    let window_raw = window.0 as usize;
    let cancel_menu = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        unsafe {
            let _ = PostMessageW(
                Some(HWND(window_raw as *mut c_void)),
                WM_CANCELMODE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    });
    let packed = u32::from(x as u16) | (u32::from(y as u16) << 16);
    unsafe {
        SendMessageW(
            window,
            WM_CONTEXTMENU,
            Some(WPARAM(window.0 as usize)),
            Some(LPARAM(packed as isize)),
        );
    }
    cancel_menu
        .join()
        .map_err(|_| format!("{label}右键菜单取消探针线程异常"))
}

#[cfg(not(windows))]
fn exercise_window_geometry(
    _receiver: &Receiver<Result<HostEvent, String>>,
    _id: &str,
    _title: &str,
    _item_count: usize,
) -> Result<(), String> {
    Err("原生窗口几何探针只支持 Windows".into())
}

#[cfg(windows)]
fn drive_existing_window(title: &str, delta_x: i32, delta_y: i32) -> Result<(), String> {
    let class_name: Vec<u16> = "Creel.DesktopHost.IpcWindow.v1"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let window_title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let window = unsafe { FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR(window_title.as_ptr())) }
        .map_err(|error| format!("没有找到桌面盒子窗口「{title}」：{error}"))?;
    let mut original_cursor = POINT::default();
    unsafe { GetCursorPos(&mut original_cursor) }
        .map_err(|error| format!("无法保存鼠标位置：{error}"))?;
    let result = (|| {
        let mut before = RECT::default();
        unsafe { GetWindowRect(window, &mut before) }
            .map_err(|error| format!("无法读取移动前矩形：{error}"))?;
        let start_x = before.left + 100;
        let start_y = before.top + 24;
        unsafe { SetCursorPos(start_x, start_y) }
            .map_err(|error| format!("无法定位移动探针鼠标：{error}"))?;
        unsafe {
            SendMessageW(window, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(LPARAM(0)));
            SetCursorPos(start_x + delta_x, start_y + delta_y)
                .map_err(|error| format!("无法移动探针鼠标：{error}"))?;
            SendMessageW(window, WM_MOUSEMOVE, Some(WPARAM(1)), Some(LPARAM(0)));
            SendMessageW(window, WM_LBUTTONUP, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        let mut after = RECT::default();
        unsafe { GetWindowRect(window, &mut after) }
            .map_err(|error| format!("无法读取移动后矩形：{error}"))?;
        if after.left != before.left.saturating_add(delta_x)
            || after.top != before.top.saturating_add(delta_y)
        {
            return Err(format!(
                "桌面盒子没有到达预期位置：({}, {}) -> ({}, {})",
                before.left, before.top, after.left, after.top
            ));
        }
        println!(
            "Moved native fence {title}: ({}, {}) -> ({}, {})",
            before.left, before.top, after.left, after.top
        );
        Ok(())
    })();
    let _ = unsafe { SetCursorPos(original_cursor.x, original_cursor.y) };
    result
}

#[cfg(not(windows))]
fn drive_existing_window(_title: &str, _delta_x: i32, _delta_y: i32) -> Result<(), String> {
    Err("原生窗口移动探针只支持 Windows".into())
}

fn run(host: &Path, folder: &Path) -> Result<(), String> {
    if !host.is_file() {
        return Err(format!("Desktop Host 不存在：{}", host.display()));
    }
    if !folder.is_dir() {
        return Err(format!("探针目录不存在：{}", folder.display()));
    }
    let mut child = Command::new(host)
        .arg("--ipc-stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 Desktop Host：{error}"))?;
    let result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Desktop Host 没有提供 stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Desktop Host 没有提供 stdout".to_string())?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let event = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str::<HostEvent>(&line).map_err(|error| error.to_string())
                });
                if sender.send(event).is_err() {
                    return;
                }
            }
        });

        send(
            &mut stdin,
            &HostCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
        )?;
        expect_ready(&receiver)?;

        let folder_name = folder
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "DCreel IPC Probe".into());
        // 使用探针专属标题，避免命中正在运行的 Creel 中映射了同一目录的盒子。
        let title = format!("DCreel IPC Probe · {folder_name}");
        let fence = HostFenceSnapshot {
            id: "ipc-probe-fence".into(),
            title: title.clone(),
            directory: folder.to_path_buf(),
            x: 96.0,
            y: 96.0,
            width: 420.0,
            height: 300.0,
            color: "sage".into(),
            content_color: "paper".into(),
            collapsed: false,
            locked: false,
            display_anchor: None,
            placement: None,
        };
        let preferences = HostPreferencesSnapshot {
            title_opacity: 0.92,
            content_opacity: 0.84,
            show_fence_border: true,
            fence_border_opacity: 0.4,
            icon_size: 46,
            ghost_mode: false,
            ghost_mode_trigger: GhostModeTrigger::Automatic,
            ghost_opacity: 0.2,
            ghost_hotkey: "Ctrl+Alt+G".into(),
            show_hidden_files: false,
            show_fence_titles: true,
        };
        send(
            &mut stdin,
            &HostCommand::Sync {
                revision: 1,
                fences: vec![fence.clone()],
                preferences: preferences.clone(),
                visible: true,
            },
        )?;
        expect_synced(&receiver, 1, 1)?;
        let item_count = folder
            .read_dir()
            .map(|entries| entries.take(2).count())
            .unwrap_or_default();
        exercise_window_geometry(&receiver, "ipc-probe-fence", &title, item_count)?;
        thread::sleep(Duration::from_millis(300));

        let hotkey_fence = HostFenceSnapshot {
            x: 128.0,
            y: 128.0,
            color: "lilac".into(),
            collapsed: true,
            ..fence.clone()
        };
        let hotkey_preferences = HostPreferencesSnapshot {
            title_opacity: 0.0,
            content_opacity: 0.0,
            ghost_mode: true,
            ghost_mode_trigger: GhostModeTrigger::Hotkey,
            ghost_hotkey: "Z+X".into(),
            show_fence_titles: false,
            ..preferences.clone()
        };
        send(
            &mut stdin,
            &HostCommand::Sync {
                revision: 2,
                fences: vec![hotkey_fence.clone()],
                preferences: hotkey_preferences.clone(),
                visible: true,
            },
        )?;
        expect_synced(&receiver, 2, 1)?;
        let window = probe_window(&title)?;
        exercise_replayed_hotkey_members(b"ZX")?;
        exercise_ghost_hotkey(window, b"ZX", "Z+X")?;

        let capture_preferences = HostPreferencesSnapshot {
            ghost_hotkey: "F23+F24".into(),
            ..hotkey_preferences
        };
        send(&mut stdin, &HostCommand::SetHotkeyCapture { active: true })?;
        send(
            &mut stdin,
            &HostCommand::Sync {
                revision: 3,
                fences: vec![hotkey_fence.clone()],
                preferences: capture_preferences.clone(),
                visible: true,
            },
        )?;
        expect_synced(&receiver, 3, 1)?;
        exercise_suspended_ghost_hotkey(window, &[0x86, 0x87], "F23+F24")?;

        send(&mut stdin, &HostCommand::SetHotkeyCapture { active: false })?;
        send(
            &mut stdin,
            &HostCommand::Sync {
                revision: 4,
                fences: vec![hotkey_fence],
                preferences: capture_preferences,
                visible: true,
            },
        )?;
        expect_synced(&receiver, 4, 1)?;
        exercise_ghost_hotkey(window, &[0x86, 0x87], "F23+F24")?;

        send(
            &mut stdin,
            &HostCommand::Sync {
                revision: 5,
                fences: Vec::new(),
                preferences,
                visible: false,
            },
        )?;
        expect_synced(&receiver, 5, 0)?;

        send(&mut stdin, &HostCommand::Shutdown)?;
        match receive(&receiver)? {
            HostEvent::Stopped => {}
            HostEvent::Error { message } => return Err(message),
            event => return Err(format!("期望 Stopped，实际收到 {event:?}")),
        }
        drop(stdin);
        let status = child
            .wait()
            .map_err(|error| format!("等待 Desktop Host 退出失败：{error}"))?;
        if !status.success() {
            return Err(format!("Desktop Host 退出码异常：{status}"));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn main() {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let Some(first) = arguments.next() else {
        eprintln!("用法：creel-desktop-host-probe.exe <host.exe> <folder>");
        std::process::exit(2);
    };
    if first == "--move-window" {
        let Some(title) = arguments.next() else {
            eprintln!("用法：creel-desktop-host-probe.exe --move-window <title> <dx> <dy>");
            std::process::exit(2);
        };
        let delta_x = arguments
            .next()
            .and_then(|value| value.to_string_lossy().parse::<i32>().ok());
        let delta_y = arguments
            .next()
            .and_then(|value| value.to_string_lossy().parse::<i32>().ok());
        let (Some(delta_x), Some(delta_y)) = (delta_x, delta_y) else {
            eprintln!("用法：creel-desktop-host-probe.exe --move-window <title> <dx> <dy>");
            std::process::exit(2);
        };
        match drive_existing_window(title.to_string_lossy().as_ref(), delta_x, delta_y) {
            Ok(()) => return,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    let host = first;
    let Some(folder) = arguments.next() else {
        eprintln!("用法：creel-desktop-host-probe.exe <host.exe> <folder>");
        std::process::exit(2);
    };
    match run(Path::new(&host), Path::new(&folder)) {
        Ok(()) => println!("DCreel Desktop Host IPC probe passed"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
