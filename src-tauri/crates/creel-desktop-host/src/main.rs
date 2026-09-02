// The native host keeps Win32 calls inside reviewed unsafe boundary functions.
#![allow(unsafe_op_in_unsafe_fn)]

use std::{path::PathBuf, time::Duration};

fn initialize_logging() {
    let directory = std::env::var_os("DCREEL_LOG_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            dirs::data_local_dir().map(|directory| directory.join("com.creel.desktop").join("logs"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("DCreel").join("logs"));
    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    match creel_logging::init(&directory, "desktop-host", level) {
        Ok(path) => {
            creel_logging::install_panic_hook("desktop-host");
            log::info!(
                target: "startup",
                "session started version={} pid={} log={}",
                env!("CARGO_PKG_VERSION"),
                std::process::id(),
                path.display()
            );
        }
        Err(error) => eprintln!("DCreel Desktop Host could not initialize logging: {error}"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Options {
    FolderProbe {
        folder: PathBuf,
        probe_duration: Option<Duration>,
    },
    IpcStdio,
}

fn parse_options(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let _program = args.next();
    let mut folder = None;
    let mut probe_duration = None;
    let mut ipc_stdio = false;
    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--folder" => {
                folder = args.next().map(PathBuf::from);
                if folder.is_none() {
                    return Err("--folder 后面需要一个目录路径".into());
                }
            }
            "--probe-seconds" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--probe-seconds 后面需要秒数".to_string())?;
                let seconds = value
                    .to_string_lossy()
                    .parse::<u64>()
                    .map_err(|_| "--probe-seconds 必须是非负整数".to_string())?;
                probe_duration = Some(Duration::from_secs(seconds));
            }
            "--ipc-stdio" => ipc_stdio = true,
            unknown => return Err(format!("无法识别参数：{unknown}")),
        }
    }
    if ipc_stdio {
        if folder.is_some() || probe_duration.is_some() {
            return Err("--ipc-stdio 不能与文件夹探针参数一起使用".into());
        }
        return Ok(Options::IpcStdio);
    }
    let folder = folder.ok_or_else(|| "缺少 --folder <真实文件夹>".to_string())?;
    if !folder.is_dir() {
        return Err(format!("目录不存在或无法访问：{}", folder.display()));
    }
    Ok(Options::FolderProbe {
        folder,
        probe_duration,
    })
}

#[cfg(windows)]
mod windows_ipc_host;

#[cfg(windows)]
mod windows_host {
    use std::time::Duration;
    use std::{
        ffi::c_void,
        fs,
        path::{Path, PathBuf},
        time::Instant,
    };
    use windows::{
        Win32::{
            Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
            Graphics::Gdi::{
                BeginPaint, CreateSolidBrush, DEFAULT_GUI_FONT, DT_END_ELLIPSIS, DT_LEFT,
                DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint,
                FillRect, GetStockObject, HGDIOBJ, InvalidateRect, PAINTSTRUCT, SelectObject,
                SetBkMode, SetTextColor, TRANSPARENT, UpdateWindow,
            },
            System::LibraryLoader::GetModuleHandleW,
            UI::WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW, EnumWindows, GW_HWNDPREV, GWLP_USERDATA,
                GetClassNameW, GetClientRect, GetMessageW, GetWindow, GetWindowLongPtrW, HWND_TOP,
                IDC_ARROW, KillTimer, LWA_ALPHA, LoadCursorW, MSG, PostQuitMessage, RegisterClassW,
                SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
                SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                TranslateMessage, WM_DESTROY, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_TIMER,
                WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
            },
        },
        core::{BOOL, Error, PCWSTR},
    };

    const CLASS_NAME: &str = "Creel.DesktopHost.Window.v1";
    const REFRESH_TIMER: usize = 1;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FolderItem {
        name: String,
        is_dir: bool,
    }

    struct HostState {
        folder: PathBuf,
        title: String,
        items: Vec<FolderItem>,
        started: Instant,
        probe_duration: Option<std::time::Duration>,
    }

    #[derive(Default)]
    struct DesktopHosts {
        progman: Option<HWND>,
        worker: Option<HWND>,
    }

    pub fn run(folder: PathBuf, probe_duration: Option<Duration>) -> Result<(), String> {
        let title = folder
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| folder.display().to_string());
        let state = Box::new(HostState {
            items: list_folder(&folder),
            folder,
            title,
            started: Instant::now(),
            probe_duration,
        });
        let raw_state = Box::into_raw(state);
        let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(Some(0)).collect();
        let window_title: Vec<u16> = "DCreel Desktop Host"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let module = unsafe { GetModuleHandleW(None) }.map_err(display_windows_error)?;
        let instance = HINSTANCE(module.0);
        let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.map_err(display_windows_error)?;
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: cursor,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return Err(display_windows_error(Error::from_thread()));
        }

        let window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(window_title.as_ptr()),
                WS_POPUP | WS_VISIBLE,
                72,
                72,
                430,
                310,
                None,
                None,
                Some(instance),
                Some(raw_state.cast::<c_void>()),
            )
        }
        .map_err(display_windows_error)?;

        unsafe {
            SetLayeredWindowAttributes(window, COLORREF(0), 238, LWA_ALPHA)
                .map_err(display_windows_error)?;
            place_on_desktop(window)?;
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(window);
            if SetTimer(Some(window), REFRESH_TIMER, 750, None) == 0 {
                return Err(display_windows_error(Error::from_thread()));
            }
        }

        println!(
            "DCreel Desktop Host {} rendering {}",
            creel_ipc::PROTOCOL_VERSION,
            state_folder(window)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<unknown>".into())
        );

        let mut message = MSG::default();
        loop {
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if result.0 == -1 {
                return Err(display_windows_error(Error::from_thread()));
            }
            if !result.as_bool() {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
                LRESULT(1)
            }
            WM_PAINT => {
                paint(window);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == REFRESH_TIMER => {
                if let Some(state) = state_mut(window) {
                    if state
                        .probe_duration
                        .is_some_and(|duration| state.started.elapsed() >= duration)
                    {
                        let _ = DestroyWindow(window);
                        return LRESULT(0);
                    }
                    let items = list_folder(&state.folder);
                    if items != state.items {
                        state.items = items;
                        let _ = InvalidateRect(Some(window), None, false);
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(Some(window), REFRESH_TIMER);
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut HostState;
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                if !pointer.is_null() {
                    drop(Box::from_raw(pointer));
                }
                DefWindowProcW(window, message, wparam, lparam)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }

    unsafe fn state_mut(window: HWND) -> Option<&'static mut HostState> {
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut HostState;
        pointer.as_mut()
    }

    fn state_folder(window: HWND) -> Option<PathBuf> {
        unsafe { state_mut(window).map(|state| state.folder.clone()) }
    }

    unsafe fn paint(window: HWND) {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(window, &mut paint);
        let mut client = RECT::default();
        if GetClientRect(window, &mut client).is_ok() {
            let panel_brush = CreateSolidBrush(rgb(245, 239, 229));
            FillRect(dc, &client, panel_brush);
            let _ = DeleteObject(HGDIOBJ(panel_brush.0));

            let header = RECT {
                left: client.left,
                top: client.top,
                right: client.right,
                bottom: 48,
            };
            let header_brush = CreateSolidBrush(rgb(226, 119, 99));
            FillRect(dc, &header, header_brush);
            let _ = DeleteObject(HGDIOBJ(header_brush.0));
            let previous_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
            SetBkMode(dc, TRANSPARENT);

            if let Some(state) = state_mut(window) {
                SetTextColor(dc, rgb(255, 255, 255));
                let mut title_rect = RECT {
                    left: 18,
                    top: 0,
                    right: client.right - 92,
                    bottom: 48,
                };
                draw_text(
                    dc,
                    &state.title,
                    &mut title_rect,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                let mut count_rect = RECT {
                    left: client.right - 88,
                    top: 0,
                    right: client.right - 14,
                    bottom: 48,
                };
                draw_text(
                    dc,
                    &format!("{} 项", state.items.len()),
                    &mut count_rect,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );

                SetTextColor(dc, rgb(61, 57, 52));
                let content_width = (client.right - 24).max(120);
                let columns = (content_width / 132).max(1);
                let rows = ((client.bottom - 60) / 48).max(1);
                let capacity = (columns * rows) as usize;
                for (index, item) in state.items.iter().take(capacity).enumerate() {
                    let column = index as i32 % columns;
                    let row = index as i32 / columns;
                    let left = 14 + column * 132;
                    let top = 58 + row * 48;
                    let mut item_rect = RECT {
                        left,
                        top,
                        right: (left + 122).min(client.right - 8),
                        bottom: top + 38,
                    };
                    let marker = if item.is_dir {
                        "[文件夹]"
                    } else {
                        "[文件]"
                    };
                    draw_text(
                        dc,
                        &format!("{marker} {}", item.name),
                        &mut item_rect,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
                    );
                }
                if state.items.len() > capacity {
                    let mut more_rect = RECT {
                        left: 14,
                        top: client.bottom - 26,
                        right: client.right - 14,
                        bottom: client.bottom - 6,
                    };
                    draw_text(
                        dc,
                        &format!("还有 {} 项…", state.items.len() - capacity),
                        &mut more_rect,
                        DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
                    );
                }
            }
            let _ = SelectObject(dc, previous_font);
        }
        let _ = EndPaint(window, &paint);
    }

    unsafe fn draw_text(
        dc: windows::Win32::Graphics::Gdi::HDC,
        value: &str,
        rect: &mut RECT,
        format: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
    ) {
        let mut text: Vec<u16> = value.encode_utf16().collect();
        DrawTextW(dc, &mut text, rect, format);
    }

    fn list_folder(folder: &Path) -> Vec<FolderItem> {
        let mut items = Vec::new();
        let Ok(entries) = fs::read_dir(folder) else {
            return items;
        };
        for entry in entries.flatten().take(500) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            items.push(FolderItem {
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir: file_type.is_dir(),
            });
        }
        items.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        items
    }

    unsafe extern "system" fn enum_desktop_hosts(window: HWND, parameter: LPARAM) -> BOOL {
        let hosts = &mut *(parameter.0 as *mut DesktopHosts);
        let mut class_name = [0u16; 64];
        let length = GetClassNameW(window, &mut class_name);
        if length > 0 {
            match String::from_utf16_lossy(&class_name[..length as usize]).as_str() {
                "Progman" => hosts.progman = Some(window),
                "WorkerW" if hosts.worker.is_none() => hosts.worker = Some(window),
                _ => {}
            }
        }
        BOOL(1)
    }

    unsafe fn place_on_desktop(window: HWND) -> Result<(), String> {
        let mut hosts = DesktopHosts::default();
        EnumWindows(
            Some(enum_desktop_hosts),
            LPARAM((&mut hosts as *mut DesktopHosts) as isize),
        )
        .map_err(display_windows_error)?;
        let host = hosts
            .progman
            .or(hosts.worker)
            .ok_or_else(|| "没有找到 Windows 桌面宿主窗口".to_string())?;
        let above = GetWindow(host, GW_HWNDPREV).unwrap_or(HWND_TOP);
        SetWindowPos(
            window,
            Some(above),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        )
        .map_err(display_windows_error)
    }

    const fn rgb(red: u32, green: u32, blue: u32) -> COLORREF {
        COLORREF(red | (green << 8) | (blue << 16))
    }

    fn display_windows_error(error: Error) -> String {
        error.to_string()
    }
}

#[cfg(windows)]
fn main() {
    initialize_logging();
    // Desktop Host 直接使用 Win32 窗口和显示器工作区坐标。显式启用 Per-Monitor
    // V2，避免 Windows 在 125%/150% 等缩放下把 SetWindowPos 坐标再次虚拟化，
    // 造成盒子越过屏幕边缘，或让吸附边界与实际像素不一致。
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    match parse_options(std::env::args_os()) {
        Ok(Options::FolderProbe {
            folder,
            probe_duration,
        }) => {
            if let Err(error) = windows_host::run(folder, probe_duration) {
                log::error!(target: "startup", "folder probe failed: {error}");
                eprintln!("DCreel Desktop Host 启动失败：{error}");
                std::process::exit(1);
            }
        }
        Ok(Options::IpcStdio) => {
            if let Err(error) = windows_ipc_host::run() {
                log::error!(target: "startup", "IPC host failed: {error}");
                eprintln!("DCreel Desktop Host IPC 启动失败：{error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            log::error!(target: "startup", "invalid command line: {error}");
            eprintln!("{error}");
            eprintln!("用法：creel-desktop-host.exe --folder <目录> [--probe-seconds <秒数>]");
            std::process::exit(2);
        }
    }
    log::info!(target: "shutdown", "Desktop Host process completed");
    creel_logging::flush();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("DCreel Desktop Host is only available on Windows");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_keeps_folder_paths_with_spaces() {
        let folder = std::env::temp_dir();
        let arguments = vec![
            "creel-desktop-host.exe".into(),
            "--folder".into(),
            folder.clone().into_os_string(),
            "--probe-seconds".into(),
            "5".into(),
        ];
        assert_eq!(
            parse_options(arguments),
            Ok(Options::FolderProbe {
                folder,
                probe_duration: Some(Duration::from_secs(5)),
            })
        );
    }

    #[test]
    fn parser_rejects_missing_folder() {
        assert_eq!(
            parse_options(vec!["creel-desktop-host.exe".into()]),
            Err("缺少 --folder <真实文件夹>".into())
        );
    }

    #[test]
    fn parser_accepts_ipc_mode_by_itself() {
        assert_eq!(
            parse_options(vec!["creel-desktop-host.exe".into(), "--ipc-stdio".into()]),
            Ok(Options::IpcStdio)
        );
    }
}
