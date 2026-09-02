use creel_ipc::{
    DisplayAnchor, FencePlacement, GhostModeTrigger, HostCommand, HostEvent, HostFenceSnapshot,
    HostPreferencesSnapshot, HostUserAction, LayoutAnchor, LayoutAxis, PROTOCOL_VERSION,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    ffi::{OsString, c_void},
    fs,
    io::{BufRead, BufReader, Write},
    mem::{ManuallyDrop, size_of},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};
use windows::{
    Win32::{
        Foundation::{
            COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
            DV_E_FORMATETC, E_INVALIDARG, E_NOTIMPL, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT,
            OLE_E_ADVISENOTSUPPORTED, POINT, POINTL, RECT, S_OK, SIZE, WPARAM,
        },
        Graphics::Gdi::{
            AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, AlphaBlend, BI_RGB, BITMAP, BITMAPINFO,
            BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint, ClientToScreen, CreateCompatibleDC,
            CreateDIBSection, CreateFontIndirectW, CreateSolidBrush, DEFAULT_GUI_FONT,
            DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE,
            DT_VCENTER, DeleteDC, DeleteObject, DrawFocusRect, DrawTextW, EndPaint,
            EnumDisplayMonitors, FillRect, GdiFlush, GetDIBits, GetMonitorInfoW, GetObjectW,
            GetStockObject, HBITMAP, HFONT, HGDIOBJ, HMONITOR, InvalidateRect, LOGFONTW,
            MONITOR_DEFAULTTONEAREST, MONITORINFOEXW, MonitorFromRect, PAINTSTRUCT, SRCCOPY,
            ScreenToClient, SelectObject, SetBkMode, SetTextColor, StretchBlt, TRANSPARENT,
            UpdateWindow,
        },
        System::{
            Com::{
                CoTaskMemFree, DATADIR_GET, DVASPECT_CONTENT, FORMATETC, IAdviseSink, IDataObject,
                IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0,
                TYMED_HGLOBAL,
            },
            LibraryLoader::GetModuleHandleW,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::{
                CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE,
                DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl,
                OleInitialize, OleUninitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
            },
            SystemServices::{MK_CONTROL, MK_LBUTTON, MK_SHIFT, MODIFIERKEYS_FLAGS},
        },
        UI::{
            Controls::{EM_SETSEL, WM_MOUSELEAVE},
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI, MDT_RAW_DPI},
            Input::KeyboardAndMouse::{
                EnableWindow, GetCapture, GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD,
                KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
                ReleaseCapture, SendInput, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
                TrackMouseEvent, VIRTUAL_KEY, VK_A, VK_APPS, VK_CONTROL, VK_DELETE, VK_DOWN,
                VK_ESCAPE, VK_F2, VK_F5, VK_F10, VK_LEFT, VK_N, VK_RETURN, VK_RIGHT, VK_SHIFT,
                VK_UP,
            },
            Shell::{
                CMF_CANRENAME, CMF_NORMAL, CMIC_MASK_PTINVOKE, CMINVOKECOMMANDINFO,
                CMINVOKECOMMANDINFOEX, Common::ITEMIDLIST, DROPFILES, DragQueryFileW, HDROP,
                IContextMenu, IContextMenu2, IContextMenu3, IShellFolder, IShellItemImageFactory,
                SHBindToParent, SHCreateItemFromParsingName, SHCreateStdEnumFmtEtc,
                SHParseDisplayName, SIIGBF, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
                SIIGBF_THUMBNAILONLY, ShellExecuteW,
            },
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CREATESTRUCTW, CS_DBLCLKS,
                CS_HREDRAW, CS_VREDRAW, CallNextHookEx, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL,
                EnumWindows, GW_HWNDPREV, GWLP_USERDATA, GetClassNameW, GetClientRect,
                GetCursorPos, GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowRect,
                GetWindowTextLengthW, GetWindowTextW, HHOOK, HMENU, HWND_TOP, IDC_ARROW,
                IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, IDOK,
                IsDialogMessageW, KBDLLHOOKSTRUCT, KillTimer, LLKHF_EXTENDED, LoadCursorW,
                MB_ICONQUESTION, MB_OK, MB_OKCANCEL, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
                MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
                SPI_GETICONTITLELOGFONT, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, SW_SHOWNORMAL,
                SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCursor, SetForegroundWindow, SetTimer,
                SetWindowLongPtrW, SetWindowPos, SetWindowTextW, SetWindowsHookExW, ShowWindow,
                SystemParametersInfoW, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
                TranslateMessage, ULW_ALPHA, UnhookWindowsHookEx, UpdateLayeredWindow,
                WH_KEYBOARD_LL, WINDOW_STYLE, WM_CAPTURECHANGED, WM_CLOSE, WM_COMMAND,
                WM_CONTEXTMENU, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_DPICHANGED,
                WM_DRAWITEM, WM_INITMENUPOPUP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
                WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MEASUREITEM, WM_MENUCHAR, WM_MOUSEMOVE,
                WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_NULL, WM_PAINT, WM_SETCURSOR,
                WM_SETFONT, WM_SETTINGCHANGE, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSW,
                WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{BOOL, Error, Free, HRESULT, Interface, PCSTR, PCWSTR, Ref, implement},
};

const CLASS_NAME: &str = "Creel.DesktopHost.IpcWindow.v1";
const CONTROLLER_TIMER: usize = 11;
const FENCE_TIMER: usize = 12;
const FENCE_TIMER_INTERVAL_MS: u32 = 250;
const TRANSPARENT_GHOST_HOVER_TIMER: usize = 13;
const TRANSPARENT_GHOST_HOVER_INTERVAL_MS: u32 = 80;
const DISPLAY_LAYOUT_TIMER: usize = 14;
const DISPLAY_LAYOUT_DEBOUNCE_MS: u32 = 240;
const HEADER_HEIGHT: i32 = 48;
const COMPACT_HEADER_HEIGHT: i32 = 16;
const SNAP_DISTANCE: i32 = 12;
const LAYOUT_COMPONENT_GAP_DIP: f64 = 64.0;
const FOREGROUND_IDLE_TIMEOUT: Duration = Duration::from_millis(2_500);
const DESKTOP_LAYER_WATCHDOG_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_ITEM_ICON_SIZE: i32 = 46;
const WM_GHOST_HOTKEY_CHORD: u32 = 0x8000 + 31;
const HOTKEY_REPLAY_EXTRA_INFO: usize = 0x4443_5245;
const RESIZE_BORDER: i32 = 8;
const MIN_FENCE_WIDTH: i32 = 244;
const MAX_FENCE_WIDTH: i32 = 1_600;
const MIN_FENCE_HEIGHT: i32 = 148;
const MAX_FENCE_HEIGHT: i32 = 1_200;
const MAX_DROP_FILES: u32 = 4_096;
const MAX_DROP_PATH_CHARS: u32 = 32_768;
const ITEM_DRAG_THRESHOLD: i32 = 6;
const WHEEL_DELTA: i32 = 120;
const CONTENT_LEFT: i32 = 12;
const CONTENT_RIGHT: i32 = 12;
const CONTENT_TOP_PADDING: i32 = 6;
const CONTENT_BOTTOM: i32 = 8;
const MAX_VISUAL_CACHE: usize = 256;
const MAX_FOLDER_ITEMS: usize = 10_000;
const MAX_KEYBOARD_OPEN_ITEMS: usize = 16;
const SHELL_MENU_ID_FIRST: u32 = 1;
const SHELL_MENU_ID_LAST: u32 = 0x7fff;
const CMIC_MASK_UNICODE_FLAG: u32 = 0x0000_4000;
const ITEM_MENU_OPEN: usize = 1;
const ITEM_MENU_REVEAL: usize = 2;
const FENCE_MENU_NEW: usize = 100;
const FENCE_MENU_OPEN: usize = 101;
const FENCE_MENU_RENAME: usize = 102;
const FENCE_MENU_COLLAPSE: usize = 103;
const FENCE_MENU_LOCK: usize = 104;
const FENCE_MENU_NEW_FOLDER: usize = 105;
const FENCE_MENU_REFRESH: usize = 106;
const FENCE_MENU_UNDO_GEOMETRY: usize = 107;
const FENCE_MENU_NEW_MAPPED: usize = 108;
const FENCE_MENU_RESET_SIZE: usize = 109;
const FENCE_MENU_COLOR_FIRST: usize = 110;
const FENCE_MENU_REMOVE: usize = 120;
const RENAME_DIALOG_OK: u16 = 1;
const RENAME_DIALOG_CANCEL: u16 = 2;

enum InputMessage {
    Command(HostCommand),
    Invalid(String),
    Closed,
}

// WindowState itself is already stored behind a Box in GWLP_USERDATA. Boxing only
// FenceState would add another allocation and pointer chase to every fence message.
#[allow(clippy::large_enum_variant)]
enum WindowState {
    Controller(ControllerState),
    Fence(FenceState),
    RenameDialog(RenameDialogState),
}

struct ControllerState {
    receiver: Receiver<InputMessage>,
    windows: HashMap<String, HWND>,
    handshake_complete: bool,
    desktop_visible: bool,
    hotkey_hidden: bool,
    hotkey_chord: Option<HotkeyChord>,
    keyboard_hook: Option<HHOOK>,
    instance: HINSTANCE,
    class_name: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HotkeyChord {
    keys: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReplayKeyboardEvent {
    virtual_key: u32,
    scan_code: u32,
    key_down: bool,
    extended: bool,
}

impl ReplayKeyboardEvent {
    #[cfg(test)]
    fn key(virtual_key: u32, key_down: bool) -> Self {
        Self {
            virtual_key,
            scan_code: 0,
            key_down,
            extended: false,
        }
    }

    fn released(self) -> Self {
        Self {
            key_down: false,
            ..self
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HotkeyHookResult {
    trigger: bool,
    suppress: bool,
    replay: Vec<ReplayKeyboardEvent>,
}

#[derive(Default)]
struct HotkeyChordTracker {
    chord: Option<HotkeyChord>,
    pressed: HashSet<u32>,
    suppressed: HashSet<u32>,
    buffered: Vec<ReplayKeyboardEvent>,
    latched: bool,
    cancelled: bool,
}

#[derive(Default)]
struct GhostKeyboardHookState {
    controller_window: HWND,
    capture_active: bool,
    tracker: HotkeyChordTracker,
}

thread_local! {
    static GHOST_KEYBOARD_HOOK_STATE: RefCell<GhostKeyboardHookState> =
        RefCell::new(GhostKeyboardHookState::default());
}

struct FenceState {
    controller_window: HWND,
    snapshot: HostFenceSnapshot,
    preferences: HostPreferencesSnapshot,
    items: Vec<FolderItem>,
    visuals: HashMap<VisualKey, Option<CachedBitmap>>,
    interaction: Option<WindowInteraction>,
    item_drag: Option<ItemDragCandidate>,
    scroll_row: usize,
    wheel_delta_remainder: i32,
    last_folder_refresh: Instant,
    last_layer_refresh: Instant,
    drop_target: Option<IDropTarget>,
    active_shell_menu: Option<ActiveShellMenu>,
    selected_paths: HashSet<PathBuf>,
    selection_anchor: Option<PathBuf>,
    focused_path: Option<PathBuf>,
    marquee: Option<MarqueeSelection>,
    mouse_inside: bool,
    undo_geometry: Option<FenceGeometry>,
    foreground_until: Option<Instant>,
    visible: bool,
}

struct ActiveShellMenu {
    menu2: Option<IContextMenu2>,
    menu3: Option<IContextMenu3>,
}

struct RenameDialogState {
    result: *mut Option<Option<String>>,
    edit: Option<HWND>,
    empty_message: String,
}

#[implement(IDropTarget)]
struct FenceDropTarget {
    window: HWND,
    accepts_files: Cell<bool>,
}

impl FenceDropTarget {
    fn new(window: HWND) -> Self {
        Self {
            window,
            accepts_files: Cell::new(false),
        }
    }
}

impl IDropTarget_Impl for FenceDropTarget_Impl {
    fn DragEnter(
        &self,
        data_object: Ref<'_, IDataObject>,
        _key_state: MODIFIERKEYS_FLAGS,
        _point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let accepts_files = supports_file_drop(data_object);
        self.accepts_files.set(accepts_files);
        unsafe { set_move_effect(effect, accepts_files) };
        Ok(())
    }

    fn DragOver(
        &self,
        _key_state: MODIFIERKEYS_FLAGS,
        _point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe { set_move_effect(effect, self.accepts_files.get()) };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.accepts_files.set(false);
        Ok(())
    }

    fn Drop(
        &self,
        data_object: Ref<'_, IDataObject>,
        _key_state: MODIFIERKEYS_FLAGS,
        _point: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.accepts_files.set(false);
        match dropped_paths(data_object) {
            Ok(paths) if !paths.is_empty() => {
                let fence_id = unsafe {
                    match state_mut(self.window) {
                        Some(WindowState::Fence(state)) => Some(state.snapshot.id.clone()),
                        _ => None,
                    }
                };
                if let Some(fence_id) = fence_id {
                    emit_event(&HostEvent::ImportFiles { fence_id, paths });
                    unsafe { set_move_effect(effect, true) };
                } else {
                    unsafe { set_move_effect(effect, false) };
                }
            }
            Ok(_) => unsafe { set_move_effect(effect, false) },
            Err(error) => {
                emit_event(&HostEvent::Notification {
                    message: format!("无法读取拖入的文件：{error}"),
                });
                unsafe { set_move_effect(effect, false) };
            }
        }
        Ok(())
    }
}

struct StorageMedium(STGMEDIUM);

impl Drop for StorageMedium {
    fn drop(&mut self) {
        unsafe { ReleaseStgMedium(&mut self.0) };
    }
}

fn file_drop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn supports_file_drop(data_object: Ref<'_, IDataObject>) -> bool {
    let Ok(data_object) = data_object.ok() else {
        return false;
    };
    unsafe { data_object.QueryGetData(&file_drop_format()).is_ok() }
}

unsafe fn set_move_effect(effect: *mut DROPEFFECT, accepts_files: bool) {
    if effect.is_null() {
        return;
    }
    let source_allows_move = ((*effect).0 & DROPEFFECT_MOVE.0) != 0;
    *effect = if accepts_files && source_allows_move {
        DROPEFFECT_MOVE
    } else {
        DROPEFFECT_NONE
    };
}

fn dropped_paths(data_object: Ref<'_, IDataObject>) -> windows::core::Result<Vec<PathBuf>> {
    let data_object = data_object.ok()?;
    let medium = StorageMedium(unsafe { data_object.GetData(&file_drop_format())? });
    if medium.0.tymed != TYMED_HGLOBAL.0 as u32 {
        return Err(Error::new(E_INVALIDARG, "拖入数据不是 Windows 文件列表"));
    }
    let global = unsafe { medium.0.u.hGlobal };
    if global.is_invalid() {
        return Err(Error::new(E_INVALIDARG, "拖入文件列表为空"));
    }
    let drop_handle = HDROP(global.0);
    let count = unsafe { DragQueryFileW(drop_handle, u32::MAX, None) };
    if count > MAX_DROP_FILES {
        return Err(Error::new(
            E_INVALIDARG,
            format!("一次最多拖入 {MAX_DROP_FILES} 个项目"),
        ));
    }
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let length = unsafe { DragQueryFileW(drop_handle, index, None) };
        if length == 0 {
            continue;
        }
        if length > MAX_DROP_PATH_CHARS {
            return Err(Error::new(E_INVALIDARG, "拖入项目的路径过长"));
        }
        let mut path = vec![0_u16; length as usize + 1];
        let written = unsafe { DragQueryFileW(drop_handle, index, Some(&mut path)) };
        if written > 0 {
            paths.push(PathBuf::from(OsString::from_wide(
                &path[..written as usize],
            )));
        }
    }
    Ok(paths)
}

#[implement(IDataObject)]
struct FileDataObject {
    paths: Vec<PathBuf>,
}

impl IDataObject_Impl for FileDataObject_Impl {
    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        if !accepts_file_drop_format(format) {
            return Err(DV_E_FORMATETC.into());
        }
        file_drop_medium(&self.paths)
    }

    fn GetDataHere(
        &self,
        _format: *const FORMATETC,
        _medium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        if accepts_file_drop_format(format) {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(&self, _input: *const FORMATETC, output: *mut FORMATETC) -> HRESULT {
        if !output.is_null() {
            unsafe { (*output).ptd = std::ptr::null_mut() };
        }
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _format: *const FORMATETC,
        _medium: *const STGMEDIUM,
        _release: BOOL,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        unsafe { SHCreateStdEnumFmtEtc(&[file_drop_format()]) }
    }

    fn DAdvise(
        &self,
        _format: *const FORMATETC,
        _flags: u32,
        _sink: Ref<'_, IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(OLE_E_ADVISENOTSUPPORTED.into())
    }
}

#[implement(IDropSource)]
struct FileDropSource;

impl IDropSource_Impl for FileDropSource_Impl {
    fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape_pressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if key_state.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

struct OwnedGlobal(HGLOBAL);

impl OwnedGlobal {
    fn into_handle(mut self) -> HGLOBAL {
        let handle = self.0;
        self.0 = HGLOBAL::default();
        handle
    }
}

struct PopupMenu(HMENU);

impl Drop for PopupMenu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}

struct OwnedPidl(*mut ITEMIDLIST);

impl Drop for OwnedPidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CoTaskMemFree(Some(self.0.cast::<c_void>())) };
        }
    }
}

impl Drop for OwnedGlobal {
    fn drop(&mut self) {
        unsafe { self.0.free() };
    }
}

fn accepts_file_drop_format(format: *const FORMATETC) -> bool {
    if format.is_null() {
        return false;
    }
    let format = unsafe { &*format };
    format.cfFormat == CF_HDROP.0
        && format.dwAspect == DVASPECT_CONTENT.0
        && format.lindex == -1
        && (format.tymed & TYMED_HGLOBAL.0 as u32) != 0
}

fn file_drop_medium(paths: &[PathBuf]) -> windows::core::Result<STGMEDIUM> {
    if paths.is_empty() {
        return Err(Error::new(E_INVALIDARG, "没有可拖出的文件"));
    }
    if paths.len() > MAX_DROP_FILES as usize {
        return Err(Error::new(
            E_INVALIDARG,
            format!("一次最多拖出 {MAX_DROP_FILES} 个项目"),
        ));
    }
    let mut encoded_paths = Vec::<u16>::new();
    for path in paths {
        encoded_paths.extend(path.as_os_str().encode_wide());
        encoded_paths.push(0);
    }
    encoded_paths.push(0);

    let header_size = std::mem::size_of::<DROPFILES>();
    let paths_size = encoded_paths
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| Error::new(E_INVALIDARG, "拖出文件列表过大"))?;
    let total_size = header_size
        .checked_add(paths_size)
        .ok_or_else(|| Error::new(E_INVALIDARG, "拖出文件列表过大"))?;
    let global = OwnedGlobal(unsafe { GlobalAlloc(GMEM_MOVEABLE, total_size)? });
    let memory = unsafe { GlobalLock(global.0) };
    if memory.is_null() {
        return Err(Error::from_thread());
    }
    unsafe {
        std::ptr::write_unaligned(
            memory.cast::<DROPFILES>(),
            DROPFILES {
                pFiles: header_size as u32,
                pt: POINT::default(),
                fNC: BOOL(0),
                fWide: BOOL(1),
            },
        );
        std::ptr::copy_nonoverlapping(
            encoded_paths.as_ptr(),
            memory.cast::<u8>().add(header_size).cast::<u16>(),
            encoded_paths.len(),
        );
        let _ = GlobalUnlock(global.0);
    }
    Ok(STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 {
            hGlobal: global.into_handle(),
        },
        pUnkForRelease: ManuallyDrop::new(None),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InteractionMode {
    Move,
    Resize(ResizeEdges),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ResizeEdges {
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
}

struct WindowInteraction {
    mode: InteractionMode,
    start_cursor: POINT,
    start_rect: RECT,
    start_geometry: FenceGeometry,
    changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FenceGeometry {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl FenceGeometry {
    fn from_snapshot(snapshot: &HostFenceSnapshot) -> Self {
        Self {
            x: snapshot.x,
            y: snapshot.y,
            width: snapshot.width,
            height: snapshot.height,
        }
    }
}

fn display_work_rect(anchor: &DisplayAnchor) -> RECT {
    RECT {
        left: anchor.work_left,
        top: anchor.work_top,
        right: anchor.work_left.saturating_add(anchor.work_width.max(1)),
        bottom: anchor.work_top.saturating_add(anchor.work_height.max(1)),
    }
}

unsafe fn display_anchor_for_monitor(monitor: HMONITOR) -> Option<(DisplayAnchor, bool)> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !GetMonitorInfoW(monitor, (&mut info as *mut MONITORINFOEXW).cast()).as_bool() {
        return None;
    }
    let work = info.monitorInfo.rcWork;
    let work_width = work.right.saturating_sub(work.left);
    let work_height = work.bottom.saturating_sub(work.top);
    if work_width <= 0 || work_height <= 0 {
        return None;
    }
    let device_end = info
        .szDevice
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(info.szDevice.len());
    let mut dpi_x = 96;
    let mut dpi_y = 96;
    let _ = GetDpiForMonitor(monitor, MDT_RAW_DPI, &mut dpi_x, &mut dpi_y);
    dpi_x = dpi_x.max(1);
    dpi_y = dpi_y.max(1);
    let mut effective_dpi_x = 96;
    let mut effective_dpi_y = 96;
    let has_effective_dpi = GetDpiForMonitor(
        monitor,
        MDT_EFFECTIVE_DPI,
        &mut effective_dpi_x,
        &mut effective_dpi_y,
    )
    .is_ok();
    Some((
        DisplayAnchor {
            device_name: String::from_utf16_lossy(&info.szDevice[..device_end]),
            work_left: work.left,
            work_top: work.top,
            work_width,
            work_height,
            dpi_x,
            dpi_y,
            effective_dpi_x: has_effective_dpi.then_some(effective_dpi_x.max(1)),
            effective_dpi_y: has_effective_dpi.then_some(effective_dpi_y.max(1)),
        },
        info.monitorInfo.dwFlags & 1 != 0,
    ))
}

unsafe extern "system" fn collect_display_anchor(
    monitor: HMONITOR,
    _dc: windows::Win32::Graphics::Gdi::HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let anchors = &mut *(data.0 as *mut Vec<(DisplayAnchor, bool)>);
    if let Some(anchor) = display_anchor_for_monitor(monitor) {
        anchors.push(anchor);
    }
    BOOL(1)
}

unsafe fn available_display_anchors() -> Vec<(DisplayAnchor, bool)> {
    let mut anchors = Vec::new();
    let _ = EnumDisplayMonitors(
        None,
        None,
        Some(collect_display_anchor),
        LPARAM((&mut anchors as *mut Vec<(DisplayAnchor, bool)>) as isize),
    );
    anchors
}

unsafe fn display_anchor_for_rect(rect: &RECT) -> Option<DisplayAnchor> {
    let monitor = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
    display_anchor_for_monitor(monitor).map(|(anchor, _)| anchor)
}

fn snapshot_window_rect(snapshot: &HostFenceSnapshot, show_fence_titles: bool) -> RECT {
    let left = logical_i32(snapshot.x);
    let top = window_y(snapshot.y, show_fence_titles);
    RECT {
        left,
        top,
        right: left.saturating_add(logical_i32(snapshot.width.max(f64::from(MIN_FENCE_WIDTH)))),
        bottom: top.saturating_add(visible_height(snapshot, show_fence_titles)),
    }
}

fn update_snapshot_window_rect(
    snapshot: &mut HostFenceSnapshot,
    rect: &RECT,
    show_fence_titles: bool,
) {
    snapshot.x = f64::from(rect.left);
    snapshot.y = f64::from(
        rect.top
            .saturating_sub(hidden_title_offset(show_fence_titles)),
    );
    snapshot.width = f64::from((rect.right - rect.left).max(1));
    if !effectively_collapsed(snapshot, show_fence_titles) {
        snapshot.height = f64::from(
            (rect.bottom - rect.top)
                .saturating_add(hidden_title_offset(show_fence_titles))
                .max(1),
        );
    }
}

fn rect_bounds(rects: &[RECT]) -> Option<RECT> {
    let first = *rects.first()?;
    Some(rects.iter().skip(1).fold(first, |bounds, rect| RECT {
        left: bounds.left.min(rect.left),
        top: bounds.top.min(rect.top),
        right: bounds.right.max(rect.right),
        bottom: bounds.bottom.max(rect.bottom),
    }))
}

fn translate_rect(rect: RECT, delta_x: i32, delta_y: i32) -> RECT {
    RECT {
        left: rect.left.saturating_add(delta_x),
        top: rect.top.saturating_add(delta_y),
        right: rect.right.saturating_add(delta_x),
        bottom: rect.bottom.saturating_add(delta_y),
    }
}

fn constrain_layout_rect(mut rect: RECT, work: &RECT, minimum_height: i32) -> RECT {
    let work_width = (work.right - work.left).max(1);
    let work_height = (work.bottom - work.top).max(1);
    let minimum_width = MIN_FENCE_WIDTH.min(work_width).max(1);
    let maximum_width = MAX_FENCE_WIDTH.min(work_width).max(minimum_width);
    let minimum_height = minimum_height.min(work_height).max(1);
    let maximum_height = MAX_FENCE_HEIGHT.min(work_height).max(minimum_height);
    let width = (rect.right - rect.left).clamp(minimum_width, maximum_width);
    let height = (rect.bottom - rect.top).clamp(minimum_height, maximum_height);
    let maximum_left = work.right.saturating_sub(width).max(work.left);
    let maximum_top = work.bottom.saturating_sub(height).max(work.top);
    rect.left = nearest_snap(rect.left, [work.left, maximum_left]).clamp(work.left, maximum_left);
    rect.top = nearest_snap(rect.top, [work.top, maximum_top]).clamp(work.top, maximum_top);
    rect.right = rect.left.saturating_add(width);
    rect.bottom = rect.top.saturating_add(height);
    rect
}

fn rect_intersection_area(left: &RECT, right: &RECT) -> i64 {
    let width = (left.right.min(right.right) - left.left.max(right.left)).max(0) as i64;
    let height = (left.bottom.min(right.bottom) - left.top.max(right.top)).max(0) as i64;
    width.saturating_mul(height)
}

fn nearest_display_index(rect: &RECT, displays: &[(DisplayAnchor, bool)]) -> Option<usize> {
    displays
        .iter()
        .enumerate()
        .map(|(index, (anchor, _))| {
            let work = display_work_rect(anchor);
            let intersection = rect_intersection_area(rect, &work);
            let rect_center_x = i64::from(rect.left) + i64::from(rect.right);
            let rect_center_y = i64::from(rect.top) + i64::from(rect.bottom);
            let work_center_x = i64::from(work.left) + i64::from(work.right);
            let work_center_y = i64::from(work.top) + i64::from(work.bottom);
            let delta_x = i128::from(rect_center_x - work_center_x);
            let delta_y = i128::from(rect_center_y - work_center_y);
            let distance = delta_x
                .saturating_mul(delta_x)
                .saturating_add(delta_y.saturating_mul(delta_y));
            (index, intersection, distance)
        })
        .min_by(|left, right| right.1.cmp(&left.1).then_with(|| left.2.cmp(&right.2)))
        .map(|(index, _, _)| index)
}

fn target_display_index(
    source: &DisplayAnchor,
    displays: &[(DisplayAnchor, bool)],
) -> Option<usize> {
    displays
        .iter()
        .position(|(anchor, _)| anchor.device_name == source.device_name)
        .or_else(|| displays.iter().position(|(_, primary)| *primary))
        .or((!displays.is_empty()).then_some(0))
}

fn rectangle_distance_squared(left: &RECT, right: &RECT) -> i64 {
    let delta_x = if left.right < right.left {
        right.left - left.right
    } else if right.right < left.left {
        left.left - right.right
    } else {
        0
    } as i64;
    let delta_y = if left.bottom < right.top {
        right.top - left.bottom
    } else if right.bottom < left.top {
        left.top - right.bottom
    } else {
        0
    } as i64;
    delta_x
        .saturating_mul(delta_x)
        .saturating_add(delta_y.saturating_mul(delta_y))
}

fn connected_layout_components(rects: &[RECT], distance: i32) -> Vec<Vec<usize>> {
    let mut visited = vec![false; rects.len()];
    let maximum_distance = i64::from(distance.max(0)).pow(2);
    let mut components = Vec::new();
    for start in 0..rects.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut component = vec![start];
        let mut cursor = 0;
        while cursor < component.len() {
            let current = component[cursor];
            for candidate in 0..rects.len() {
                if !visited[candidate]
                    && rectangle_distance_squared(&rects[current], &rects[candidate])
                        <= maximum_distance
                {
                    visited[candidate] = true;
                    component.push(candidate);
                }
            }
            cursor += 1;
        }
        components.push(component);
    }
    components
}

fn layout_position_ratio(position: i32, size: i32, work_position: i32, work_size: i32) -> f64 {
    let available = work_size.saturating_sub(size);
    if available <= 0 {
        0.0
    } else {
        (f64::from(position.saturating_sub(work_position)) / f64::from(available)).clamp(0.0, 1.0)
    }
}

fn effective_dpi(anchor: &DisplayAnchor, horizontal: bool) -> u32 {
    if horizontal {
        anchor.effective_dpi_x.unwrap_or(anchor.dpi_x).max(1)
    } else {
        anchor.effective_dpi_y.unwrap_or(anchor.dpi_y).max(1)
    }
}

fn dip_scale(anchor: &DisplayAnchor, horizontal: bool) -> f64 {
    f64::from(effective_dpi(anchor, horizontal)) / 96.0
}

fn remap_scale(source: &DisplayAnchor, target: &DisplayAnchor, horizontal: bool) -> f64 {
    let effective = if horizontal {
        source.effective_dpi_x.zip(target.effective_dpi_x)
    } else {
        source.effective_dpi_y.zip(target.effective_dpi_y)
    };
    let (source_dpi, target_dpi) = effective.unwrap_or({
        if horizontal {
            (source.dpi_x, target.dpi_x)
        } else {
            (source.dpi_y, target.dpi_y)
        }
    });
    (f64::from(target_dpi.max(1)) / f64::from(source_dpi.max(1))).clamp(0.5, 3.0)
}

fn layout_component_distance(anchor: &DisplayAnchor) -> i32 {
    (LAYOUT_COMPONENT_GAP_DIP * dip_scale(anchor, true).max(dip_scale(anchor, false)))
        .round()
        .clamp(24.0, 384.0) as i32
}

fn component_layout_axis(
    start: i32,
    end: i32,
    work_start: i32,
    work_end: i32,
    scale: f64,
) -> LayoutAxis {
    let start_margin = start.saturating_sub(work_start).max(0);
    let end_margin = work_end.saturating_sub(end).max(0);
    if start_margin <= SNAP_DISTANCE && start_margin <= end_margin {
        LayoutAxis {
            anchor: LayoutAnchor::Start,
            value: f64::from(start_margin) / scale.max(f64::EPSILON),
        }
    } else if end_margin <= SNAP_DISTANCE {
        LayoutAxis {
            anchor: LayoutAnchor::End,
            value: f64::from(end_margin) / scale.max(f64::EPSILON),
        }
    } else {
        LayoutAxis {
            anchor: LayoutAnchor::Proportional,
            value: layout_position_ratio(
                start,
                end.saturating_sub(start),
                work_start,
                work_end.saturating_sub(work_start),
            ),
        }
    }
}

fn assign_layout_placements(snapshots: &mut [HostFenceSnapshot], show_fence_titles: bool) {
    let mut assigned = vec![false; snapshots.len()];
    for start in 0..snapshots.len() {
        if assigned[start] {
            continue;
        }
        let Some(anchor) = snapshots[start].display_anchor.clone() else {
            snapshots[start].placement = None;
            assigned[start] = true;
            continue;
        };
        let indices = (start..snapshots.len())
            .filter(|index| {
                !assigned[*index] && snapshots[*index].display_anchor.as_ref() == Some(&anchor)
            })
            .collect::<Vec<_>>();
        for index in &indices {
            assigned[*index] = true;
        }
        let rects = indices
            .iter()
            .map(|index| snapshot_window_rect(&snapshots[*index], show_fence_titles))
            .collect::<Vec<_>>();
        let components = connected_layout_components(&rects, layout_component_distance(&anchor));
        let work = display_work_rect(&anchor);
        let scale_x = dip_scale(&anchor, true);
        let scale_y = dip_scale(&anchor, false);
        for component in components {
            let component_rects = component
                .iter()
                .map(|local_index| rects[*local_index])
                .collect::<Vec<_>>();
            let Some(bounds) = rect_bounds(&component_rects) else {
                continue;
            };
            let horizontal =
                component_layout_axis(bounds.left, bounds.right, work.left, work.right, scale_x);
            let vertical =
                component_layout_axis(bounds.top, bounds.bottom, work.top, work.bottom, scale_y);
            let group_id = component
                .iter()
                .map(|local_index| snapshots[indices[*local_index]].id.as_str())
                .min()
                .unwrap_or_default()
                .to_string();
            for local_index in component {
                let index = indices[local_index];
                let rect = rects[local_index];
                snapshots[index].placement = Some(FencePlacement {
                    group_id: group_id.clone(),
                    horizontal,
                    vertical,
                    offset_x_dip: f64::from(rect.left.saturating_sub(bounds.left)) / scale_x,
                    offset_y_dip: f64::from(rect.top.saturating_sub(bounds.top)) / scale_y,
                });
            }
        }
    }
}

fn connected_layout_components_with_placements(
    snapshots: &[HostFenceSnapshot],
    indices: &[usize],
    rects: &[RECT],
    distance: i32,
) -> Vec<Vec<usize>> {
    let mut visited = vec![false; rects.len()];
    let maximum_distance = i64::from(distance.max(0)).pow(2);
    let mut components = Vec::new();
    for start in 0..rects.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut component = vec![start];
        let mut cursor = 0;
        while cursor < component.len() {
            let current = component[cursor];
            let current_group = snapshots[indices[current]]
                .placement
                .as_ref()
                .map(|placement| placement.group_id.as_str());
            for candidate in 0..rects.len() {
                if visited[candidate] {
                    continue;
                }
                let candidate_group = snapshots[indices[candidate]]
                    .placement
                    .as_ref()
                    .map(|placement| placement.group_id.as_str());
                let same_saved_group = current_group.is_some() && current_group == candidate_group;
                if same_saved_group
                    || rectangle_distance_squared(&rects[current], &rects[candidate])
                        <= maximum_distance
                {
                    visited[candidate] = true;
                    component.push(candidate);
                }
            }
            cursor += 1;
        }
        components.push(component);
    }
    components
}

fn layout_axis_ratio(axis: LayoutAxis) -> f64 {
    match axis.anchor {
        LayoutAnchor::Start => 0.0,
        LayoutAnchor::End => 1.0,
        LayoutAnchor::Proportional => axis.value.clamp(0.0, 1.0),
    }
}

fn target_axis_position(
    axis: LayoutAxis,
    work_start: i32,
    work_size: i32,
    group_size: i32,
    scale: f64,
) -> i32 {
    let available = work_size.saturating_sub(group_size).max(0);
    match axis.anchor {
        LayoutAnchor::Start => {
            work_start.saturating_add((axis.value.max(0.0) * scale).round() as i32)
        }
        LayoutAnchor::End => work_start
            .saturating_add(available)
            .saturating_sub((axis.value.max(0.0) * scale).round() as i32),
        LayoutAnchor::Proportional => work_start
            .saturating_add((axis.value.clamp(0.0, 1.0) * f64::from(available)).round() as i32),
    }
}

fn geometry_changed_event(snapshot: &HostFenceSnapshot) -> HostEvent {
    HostEvent::GeometryChanged {
        id: snapshot.id.clone(),
        x: snapshot.x,
        y: snapshot.y,
        width: snapshot.width,
        height: snapshot.height,
        display_anchor: snapshot.display_anchor.clone(),
        placement: snapshot.placement.clone(),
    }
}

fn rect_inside(rect: &RECT, work: &RECT) -> bool {
    rect.left >= work.left
        && rect.top >= work.top
        && rect.right <= work.right
        && rect.bottom <= work.bottom
}

fn group_intersects(rects: &[RECT], occupied: &[RECT]) -> bool {
    rects
        .iter()
        .any(|rect| occupied.iter().any(|other| rects_intersect(rect, other)))
}

fn move_group_away_from_occupied(rects: &mut [RECT], occupied: &[RECT], work: &RECT) {
    let Some(bounds) = rect_bounds(rects) else {
        return;
    };
    if !group_intersects(rects, occupied) {
        return;
    }
    let group_width = bounds.right - bounds.left;
    let group_height = bounds.bottom - bounds.top;
    if group_width > work.right - work.left || group_height > work.bottom - work.top {
        return;
    }
    let mut horizontal = vec![bounds.left, work.left, work.right - group_width];
    let mut vertical = vec![bounds.top, work.top, work.bottom - group_height];
    for other in occupied {
        horizontal.push(other.left - group_width);
        horizontal.push(other.right);
        vertical.push(other.top - group_height);
        vertical.push(other.bottom);
    }
    horizontal.sort_unstable();
    horizontal.dedup();
    vertical.sort_unstable();
    vertical.dedup();
    let best = horizontal
        .into_iter()
        .flat_map(|left| vertical.iter().copied().map(move |top| (left, top)))
        .filter_map(|(left, top)| {
            let delta_x = left.saturating_sub(bounds.left);
            let delta_y = top.saturating_sub(bounds.top);
            let candidate = rects
                .iter()
                .map(|rect| translate_rect(*rect, delta_x, delta_y))
                .collect::<Vec<_>>();
            (candidate.iter().all(|rect| rect_inside(rect, work))
                && !group_intersects(&candidate, occupied))
            .then_some((
                delta_x
                    .unsigned_abs()
                    .saturating_add(delta_y.unsigned_abs()),
                candidate,
            ))
        })
        .min_by_key(|(distance, _)| *distance);
    if let Some((_, candidate)) = best {
        rects.copy_from_slice(&candidate);
    }
}

// The independent scale and anchor ratios are kept explicit because they use
// different axes and units; bundling them would make the geometry code opaque.
#[allow(clippy::too_many_arguments)]
fn separate_mapped_rects(
    rects: &mut [RECT],
    source_rects: &[RECT],
    work: &RECT,
    minimum_height: i32,
    scale_x: f64,
    scale_y: f64,
    x_ratio: f64,
    y_ratio: f64,
) {
    let mut order = (0..rects.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        let left_rect = &source_rects[*left];
        let right_rect = &source_rects[*right];
        let horizontal = if x_ratio >= 0.5 {
            right_rect.right.cmp(&left_rect.right)
        } else {
            left_rect.left.cmp(&right_rect.left)
        };
        let vertical = if y_ratio >= 0.5 {
            right_rect.bottom.cmp(&left_rect.bottom)
        } else {
            left_rect.top.cmp(&right_rect.top)
        };
        horizontal.then(vertical)
    });
    let desired = rects.to_vec();
    let mut placed = Vec::<usize>::new();
    for index in order {
        let mut candidates = vec![desired[index]];
        let width = desired[index].right - desired[index].left;
        let height = desired[index].bottom - desired[index].top;
        for previous in placed.iter().copied() {
            let source = &source_rects[index];
            let other_source = &source_rects[previous];
            let other = &rects[previous];
            if source.left >= other_source.right {
                let gap = (f64::from(source.left - other_source.right) * scale_x).round() as i32;
                candidates.push(RECT {
                    left: other.right.saturating_add(gap),
                    right: other.right.saturating_add(gap).saturating_add(width),
                    ..desired[index]
                });
            }
            if source.right <= other_source.left {
                let gap = (f64::from(other_source.left - source.right) * scale_x).round() as i32;
                candidates.push(RECT {
                    left: other.left.saturating_sub(gap).saturating_sub(width),
                    right: other.left.saturating_sub(gap),
                    ..desired[index]
                });
            }
            if source.top >= other_source.bottom {
                let gap = (f64::from(source.top - other_source.bottom) * scale_y).round() as i32;
                candidates.push(RECT {
                    top: other.bottom.saturating_add(gap),
                    bottom: other.bottom.saturating_add(gap).saturating_add(height),
                    ..desired[index]
                });
            }
            if source.bottom <= other_source.top {
                let gap = (f64::from(other_source.top - source.bottom) * scale_y).round() as i32;
                candidates.push(RECT {
                    top: other.top.saturating_sub(gap).saturating_sub(height),
                    bottom: other.top.saturating_sub(gap),
                    ..desired[index]
                });
            }
            candidates.push(RECT {
                left: other.right,
                right: other.right.saturating_add(width),
                ..desired[index]
            });
            candidates.push(RECT {
                left: other.left.saturating_sub(width),
                right: other.left,
                ..desired[index]
            });
            candidates.push(RECT {
                top: other.bottom,
                bottom: other.bottom.saturating_add(height),
                ..desired[index]
            });
            candidates.push(RECT {
                top: other.top.saturating_sub(height),
                bottom: other.top,
                ..desired[index]
            });
        }
        let best = candidates
            .into_iter()
            .map(|candidate| constrain_layout_rect(candidate, work, minimum_height))
            .filter(|candidate| {
                placed
                    .iter()
                    .all(|previous| !rects_intersect(candidate, &rects[*previous]))
            })
            .min_by_key(|candidate| {
                candidate
                    .left
                    .abs_diff(desired[index].left)
                    .saturating_add(candidate.top.abs_diff(desired[index].top))
            });
        if let Some(best) = best {
            rects[index] = best;
        }
        placed.push(index);
    }
}

fn remap_layout_component(
    snapshots: &[HostFenceSnapshot],
    indices: &[usize],
    source: &DisplayAnchor,
    target: &DisplayAnchor,
    show_fence_titles: bool,
    occupied: &[RECT],
) -> Vec<RECT> {
    let source_rects = indices
        .iter()
        .map(|index| snapshot_window_rect(&snapshots[*index], show_fence_titles))
        .collect::<Vec<_>>();
    let old_bounds = rect_bounds(&source_rects).unwrap_or_else(|| display_work_rect(source));
    let scale_x = remap_scale(source, target, true);
    let scale_y = remap_scale(source, target, false);
    let work = display_work_rect(target);
    let work_width = (work.right - work.left).max(1);
    let work_height = (work.bottom - work.top).max(1);
    let minimum_height = MIN_FENCE_HEIGHT.saturating_sub(hidden_title_offset(show_fence_titles));
    let semantic = indices
        .first()
        .and_then(|index| snapshots[*index].placement.as_ref())
        .filter(|first| {
            !first.group_id.is_empty()
                && indices.iter().all(|index| {
                    snapshots[*index]
                        .placement
                        .as_ref()
                        .is_some_and(|placement| {
                            placement.group_id == first.group_id
                                && placement.horizontal == first.horizontal
                                && placement.vertical == first.vertical
                        })
                })
        })
        .cloned();
    if let Some(semantic) = semantic {
        let target_scale_x = dip_scale(target, true);
        let target_scale_y = dip_scale(target, false);
        let mut mapped = indices
            .iter()
            .zip(&source_rects)
            .map(|(index, rect)| {
                let placement = snapshots[*index].placement.as_ref().unwrap();
                let left = (placement.offset_x_dip * target_scale_x).round() as i32;
                let top = (placement.offset_y_dip * target_scale_y).round() as i32;
                let width = (f64::from(rect.right - rect.left) * scale_x)
                    .round()
                    .clamp(1.0, f64::from(work_width)) as i32;
                let height = (f64::from(rect.bottom - rect.top) * scale_y)
                    .round()
                    .clamp(1.0, f64::from(work_height)) as i32;
                RECT {
                    left,
                    top,
                    right: left.saturating_add(width),
                    bottom: top.saturating_add(height),
                }
            })
            .collect::<Vec<_>>();
        let mapped_bounds = rect_bounds(&mapped).unwrap_or_default();
        let mapped_width = mapped_bounds.right - mapped_bounds.left;
        let mapped_height = mapped_bounds.bottom - mapped_bounds.top;
        let desired_left = target_axis_position(
            semantic.horizontal,
            work.left,
            work_width,
            mapped_width,
            target_scale_x,
        );
        let desired_top = target_axis_position(
            semantic.vertical,
            work.top,
            work_height,
            mapped_height,
            target_scale_y,
        );
        let delta_x = desired_left.saturating_sub(mapped_bounds.left);
        let delta_y = desired_top.saturating_sub(mapped_bounds.top);
        for rect in &mut mapped {
            *rect = translate_rect(*rect, delta_x, delta_y);
            *rect = constrain_layout_rect(*rect, &work, minimum_height);
        }
        separate_mapped_rects(
            &mut mapped,
            &source_rects,
            &work,
            minimum_height,
            scale_x,
            scale_y,
            layout_axis_ratio(semantic.horizontal),
            layout_axis_ratio(semantic.vertical),
        );
        move_group_away_from_occupied(&mut mapped, occupied, &work);
        return mapped;
    }
    let mut mapped = source_rects
        .iter()
        .map(|rect| {
            let left = ((rect.left - old_bounds.left) as f64 * scale_x).round() as i32;
            let top = ((rect.top - old_bounds.top) as f64 * scale_y).round() as i32;
            let width = (f64::from(rect.right - rect.left) * scale_x)
                .round()
                .clamp(1.0, f64::from(work_width)) as i32;
            let height = (f64::from(rect.bottom - rect.top) * scale_y)
                .round()
                .clamp(1.0, f64::from(work_height)) as i32;
            RECT {
                left,
                top,
                right: left.saturating_add(width),
                bottom: top.saturating_add(height),
            }
        })
        .collect::<Vec<_>>();
    let scaled_bounds = rect_bounds(&mapped).unwrap_or_default();
    let source_x_ratio = layout_position_ratio(
        old_bounds.left,
        old_bounds.right - old_bounds.left,
        source.work_left,
        source.work_width,
    );
    let source_y_ratio = layout_position_ratio(
        old_bounds.top,
        old_bounds.bottom - old_bounds.top,
        source.work_top,
        source.work_height,
    );
    let mapped_width = scaled_bounds.right - scaled_bounds.left;
    let mapped_height = scaled_bounds.bottom - scaled_bounds.top;
    let desired_left = work.left.saturating_add(
        (source_x_ratio * f64::from((work_width - mapped_width).max(0))).round() as i32,
    );
    let desired_top = work.top.saturating_add(
        (source_y_ratio * f64::from((work_height - mapped_height).max(0))).round() as i32,
    );
    let delta_x = desired_left.saturating_sub(scaled_bounds.left);
    let delta_y = desired_top.saturating_sub(scaled_bounds.top);
    for rect in &mut mapped {
        *rect = translate_rect(*rect, delta_x, delta_y);
        *rect = constrain_layout_rect(*rect, &work, minimum_height);
    }
    separate_mapped_rects(
        &mut mapped,
        &source_rects,
        &work,
        minimum_height,
        scale_x,
        scale_y,
        source_x_ratio,
        source_y_ratio,
    );
    move_group_away_from_occupied(&mut mapped, occupied, &work);
    mapped
}

fn reconcile_display_layout(
    mut snapshots: Vec<HostFenceSnapshot>,
    displays: &[(DisplayAnchor, bool)],
    show_fence_titles: bool,
) -> (Vec<HostFenceSnapshot>, Vec<HostEvent>) {
    if displays.is_empty() {
        return (snapshots, Vec::new());
    }
    let original = snapshots.clone();
    let mut pending = Vec::new();
    let mut occupied = vec![Vec::<RECT>::new(); displays.len()];

    for (index, snapshot) in snapshots.iter_mut().enumerate() {
        let rect = snapshot_window_rect(snapshot, show_fence_titles);
        let Some(saved) = snapshot.display_anchor.clone() else {
            let display_index = nearest_display_index(&rect, displays).unwrap_or(0);
            let target = &displays[display_index].0;
            let work = display_work_rect(target);
            let minimum_height = if effectively_collapsed(snapshot, show_fence_titles) {
                HEADER_HEIGHT
            } else {
                MIN_FENCE_HEIGHT.saturating_sub(hidden_title_offset(show_fence_titles))
            };
            let rect = constrain_layout_rect(rect, &work, minimum_height);
            update_snapshot_window_rect(snapshot, &rect, show_fence_titles);
            snapshot.display_anchor = Some(target.clone());
            occupied[display_index].push(rect);
            continue;
        };
        let display_index = target_display_index(&saved, displays).unwrap_or(0);
        let target = &displays[display_index].0;
        if saved == *target {
            let work = display_work_rect(target);
            let minimum_height = if effectively_collapsed(snapshot, show_fence_titles) {
                HEADER_HEIGHT
            } else {
                MIN_FENCE_HEIGHT.saturating_sub(hidden_title_offset(show_fence_titles))
            };
            let rect = constrain_layout_rect(rect, &work, minimum_height);
            update_snapshot_window_rect(snapshot, &rect, show_fence_titles);
            occupied[display_index].push(rect);
        } else {
            pending.push(index);
        }
    }

    let mut visited = vec![false; snapshots.len()];
    for start in pending.iter().copied() {
        if visited[start] {
            continue;
        }
        let source = snapshots[start].display_anchor.clone().unwrap();
        let same_source = pending
            .iter()
            .copied()
            .filter(|index| {
                !visited[*index] && snapshots[*index].display_anchor.as_ref() == Some(&source)
            })
            .collect::<Vec<_>>();
        let source_rects = same_source
            .iter()
            .map(|index| snapshot_window_rect(&snapshots[*index], show_fence_titles))
            .collect::<Vec<_>>();
        let grouping_distance = layout_component_distance(&source);
        let components = connected_layout_components_with_placements(
            &snapshots,
            &same_source,
            &source_rects,
            grouping_distance,
        );
        let display_index = target_display_index(&source, displays).unwrap_or(0);
        let target = displays[display_index].0.clone();
        for component in components {
            let indices = component
                .iter()
                .map(|local_index| same_source[*local_index])
                .collect::<Vec<_>>();
            let mapped = remap_layout_component(
                &snapshots,
                &indices,
                &source,
                &target,
                show_fence_titles,
                &occupied[display_index],
            );
            for (index, rect) in indices.iter().copied().zip(mapped) {
                update_snapshot_window_rect(&mut snapshots[index], &rect, show_fence_titles);
                snapshots[index].display_anchor = Some(target.clone());
                occupied[display_index].push(rect);
                visited[index] = true;
            }
        }
    }

    assign_layout_placements(&mut snapshots, show_fence_titles);

    let events = original
        .iter()
        .zip(&snapshots)
        .filter(|(before, after)| *before != *after)
        .map(|(_, snapshot)| geometry_changed_event(snapshot))
        .collect();
    (snapshots, events)
}

struct ItemDragCandidate {
    path: PathBuf,
    start: POINT,
    collapse_on_release: bool,
}

struct MarqueeSelection {
    start: POINT,
    current: POINT,
    base_selection: HashSet<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SelectionModifiers {
    control: bool,
    shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavigationDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FolderItem {
    name: String,
    path: PathBuf,
    is_dir: bool,
    modified_at_nanos: u128,
    length: u64,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct VisualKey {
    path: PathBuf,
    size: u32,
}

struct CachedBitmap {
    handle: HBITMAP,
    width: i32,
    height: i32,
    kind: ShellVisualKind,
    uses_alpha: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellVisualKind {
    Thumbnail,
    Icon,
}

impl Drop for CachedBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.handle.0));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ItemCell {
    index: usize,
    bounds: RECT,
    icon: RECT,
    label: RECT,
}

struct DesktopTextOverlay {
    value: String,
    rect: RECT,
}

#[derive(Clone, Copy)]
struct DesktopVisualOverlay {
    handle: HBITMAP,
    width: i32,
    height: i32,
    kind: ShellVisualKind,
    uses_alpha: bool,
    bounds: RECT,
}

#[derive(Clone, Copy)]
struct DesktopRectangleOverlay {
    bounds: RECT,
    fill_color: COLORREF,
    fill_alpha: u8,
    border_color: COLORREF,
    border_alpha: u8,
}

#[derive(Default)]
struct DesktopItemOverlays {
    rectangles: Vec<DesktopRectangleOverlay>,
    visuals: Vec<DesktopVisualOverlay>,
    texts: Vec<DesktopTextOverlay>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GridMetrics {
    columns: usize,
    row_height: i32,
    visible_rows: usize,
    total_rows: usize,
    max_scroll_row: usize,
}

#[derive(Default)]
struct DesktopHosts {
    progman: Option<HWND>,
    worker: Option<HWND>,
}

pub fn run() -> Result<(), String> {
    log::info!(target: "ipc", "IPC host initialization started");
    let _com_apartment = ComApartment::initialize()?;
    log::info!(target: "ipc", "COM apartment initialized");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        log::debug!(target: "ipc", "stdin reader started");
        let stdin = std::io::stdin();
        for line in BufReader::new(stdin.lock()).lines() {
            match line {
                Ok(line) if line.trim().is_empty() => continue,
                Ok(line) => match serde_json::from_str::<HostCommand>(&line) {
                    Ok(command) => {
                        if sender.send(InputMessage::Command(command)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        log::warn!(target: "ipc", "invalid command payload: {error}");
                        if sender
                            .send(InputMessage::Invalid(format!("无效 IPC 消息：{error}")))
                            .is_err()
                        {
                            return;
                        }
                    }
                },
                Err(error) => {
                    log::error!(target: "ipc", "stdin read failed: {error}");
                    let _ = sender.send(InputMessage::Invalid(format!("无法读取 IPC：{error}")));
                    break;
                }
            }
        }
        log::info!(target: "ipc", "stdin closed");
        let _ = sender.send(InputMessage::Closed);
    });

    let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(Some(0)).collect();
    let controller_title: Vec<u16> = "DCreel Desktop Host IPC"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let module = unsafe { GetModuleHandleW(None) }.map_err(display_windows_error)?;
    let instance = HINSTANCE(module.0);
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.map_err(display_windows_error)?;
    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&window_class) } == 0 {
        return Err(display_windows_error(Error::from_thread()));
    }
    log::debug!(target: "ipc", "controller window class registered");

    let controller = Box::new(WindowState::Controller(ControllerState {
        receiver,
        windows: HashMap::new(),
        handshake_complete: false,
        desktop_visible: false,
        hotkey_hidden: false,
        hotkey_chord: None,
        keyboard_hook: None,
        instance,
        class_name: class_name.clone(),
    }));
    let raw_controller = Box::into_raw(controller);
    let controller_window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(controller_title.as_ptr()),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            Some(raw_controller.cast::<c_void>()),
        )
    }
    .map_err(display_windows_error)?;
    log::info!(target: "ipc", "controller window created");
    let keyboard_hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(ghost_keyboard_hook), Some(instance), 0) }
            .map_err(display_windows_error)?;
    if let Some(WindowState::Controller(controller)) = unsafe { state_mut(controller_window) } {
        controller.keyboard_hook = Some(keyboard_hook);
    }
    GHOST_KEYBOARD_HOOK_STATE.with(|state| {
        state.borrow_mut().controller_window = controller_window;
    });
    log::info!(target: "hotkey", "low-level keyboard hook installed");
    if unsafe { SetTimer(Some(controller_window), CONTROLLER_TIMER, 50, None) } == 0 {
        return Err(display_windows_error(Error::from_thread()));
    }

    log::info!(target: "ipc", "host message loop started");
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
    log::info!(target: "ipc", "host message loop stopped");
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
        WM_GHOST_HOTKEY_CHORD if matches!(state_mut(window), Some(WindowState::Controller(_))) => {
            toggle_hotkey_fences(window);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DEVICECHANGE | WM_SETTINGCHANGE | WM_DPICHANGED
            if matches!(
                state_mut(window),
                Some(WindowState::Controller(_) | WindowState::Fence(_))
            ) =>
        {
            schedule_display_layout_reconcile(window);
            LRESULT(0)
        }
        WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR => {
            forward_active_shell_menu(window, message, wparam, lparam)
                .unwrap_or_else(|| DefWindowProcW(window, message, wparam, lparam))
        }
        WM_SETCURSOR if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            if update_fence_cursor(window) {
                LRESULT(1)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_CONTEXTMENU if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            activate_fence_temporarily(window);
            show_fence_context_menu(window, context_menu_point(window, lparam));
            LRESULT(0)
        }
        WM_COMMAND if matches!(state_mut(window), Some(WindowState::RenameDialog(_))) => {
            if handle_rename_dialog_command(window, (wparam.0 & 0xffff) as u16) {
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_CLOSE if matches!(state_mut(window), Some(WindowState::RenameDialog(_))) => {
            finish_rename_dialog(window, None);
            LRESULT(0)
        }
        WM_KEYDOWN if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            activate_fence_temporarily(window);
            if handle_fence_keydown(window, wparam) {
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_LBUTTONDBLCLK if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            activate_fence_temporarily(window);
            if open_fence_item_at(window, client_point(lparam)) {
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_LBUTTONDOWN if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            activate_fence_temporarily(window);
            let point = client_point(lparam);
            if begin_fence_interaction(window)
                || begin_item_drag(window, point, wparam)
                || begin_marquee_selection(window, point, wparam)
            {
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_MOUSEMOVE if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            extend_fence_foreground_activity(window);
            let hover_changed = enter_fence_hover(window);
            if update_fence_interaction(window)
                || update_item_drag(window, client_point(lparam), wparam)
                || update_marquee_selection(window, client_point(lparam), wparam)
                || hover_changed
            {
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_MOUSELEAVE if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            leave_fence_hover(window);
            LRESULT(0)
        }
        WM_MOUSEWHEEL if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            activate_fence_temporarily(window);
            if scroll_fence(window, wheel_delta(wparam)) {
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_LBUTTONUP if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            let handled = finish_fence_interaction(window)
                || finish_item_pointer(window)
                || finish_marquee_selection(window);
            if handled {
                if GetCapture() == window {
                    let _ = ReleaseCapture();
                }
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_CAPTURECHANGED if matches!(state_mut(window), Some(WindowState::Fence(_))) => {
            let _ = finish_fence_interaction(window);
            let _ = cancel_item_drag(window);
            let _ = cancel_marquee_selection(window);
            LRESULT(0)
        }
        WM_PAINT => {
            if matches!(state_mut(window), Some(WindowState::Fence(_))) {
                paint_fence(window);
                LRESULT(0)
            } else if matches!(state_mut(window), Some(WindowState::RenameDialog(_))) {
                paint_rename_dialog(window);
                LRESULT(0)
            } else {
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
        WM_TIMER if wparam.0 == CONTROLLER_TIMER => {
            process_controller_messages(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == DISPLAY_LAYOUT_TIMER => {
            let _ = KillTimer(Some(window), DISPLAY_LAYOUT_TIMER);
            reconcile_live_display_layout(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == FENCE_TIMER => {
            refresh_fence(window, false);
            // “显示桌面”等 Shell 操作可能重排顶层窗口。定期把盒子重新放回
            // 桌面宿主正上方，避免它落到墙纸后面；这里不使用 TOPMOST，
            // 因而盒子仍会保持在普通应用窗口下方。
            if should_refresh_fence_layers(window) {
                let _ = place_fence_layers(window);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TRANSPARENT_GHOST_HOVER_TIMER => {
            poll_transparent_ghost_hover(window);
            LRESULT(0)
        }
        WM_DESTROY => {
            match state_mut(window) {
                Some(WindowState::Controller(_)) => {
                    let _ = KillTimer(Some(window), CONTROLLER_TIMER);
                    let _ = KillTimer(Some(window), DISPLAY_LAYOUT_TIMER);
                    PostQuitMessage(0);
                }
                Some(WindowState::Fence(state)) => {
                    let _ = KillTimer(Some(window), FENCE_TIMER);
                    let _ = KillTimer(Some(window), TRANSPARENT_GHOST_HOVER_TIMER);
                    let _ = RevokeDragDrop(window);
                    state.drop_target.take();
                    state.active_shell_menu.take();
                }
                Some(WindowState::RenameDialog(state))
                    if !state.result.is_null() && (*state.result).is_none() =>
                {
                    *state.result = Some(None);
                }
                Some(WindowState::RenameDialog(_)) => {}
                None => {}
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut WindowState;
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            if !pointer.is_null() {
                drop(Box::from_raw(pointer));
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn schedule_display_layout_reconcile(window: HWND) {
    let controller_window = match state_mut(window) {
        Some(WindowState::Controller(_)) => window,
        Some(WindowState::Fence(state)) => state.controller_window,
        _ => return,
    };
    let _ = SetTimer(
        Some(controller_window),
        DISPLAY_LAYOUT_TIMER,
        DISPLAY_LAYOUT_DEBOUNCE_MS,
        None,
    );
}

unsafe fn forward_active_shell_menu(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let active = match state_mut(window) {
        Some(WindowState::Fence(state)) => state.active_shell_menu.as_ref()?,
        _ => return None,
    };
    if let Some(menu3) = active.menu3.as_ref() {
        let mut result = LRESULT(0);
        if menu3
            .HandleMenuMsg2(message, wparam, lparam, Some(&mut result))
            .is_ok()
        {
            return Some(result);
        }
    }
    if message != WM_MENUCHAR
        && let Some(menu2) = active.menu2.as_ref()
        && menu2.HandleMenuMsg(message, wparam, lparam).is_ok()
    {
        return Some(LRESULT(0));
    }
    None
}

unsafe fn process_controller_messages(controller_window: HWND) {
    let messages: Vec<InputMessage> = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => controller.receiver.try_iter().collect(),
        _ => return,
    };
    for message in messages {
        match message {
            InputMessage::Command(command) => apply_command(controller_window, command),
            InputMessage::Invalid(message) => emit_event(&HostEvent::Error { message }),
            InputMessage::Closed => {
                shutdown(controller_window);
                return;
            }
        }
    }
}

unsafe fn reconcile_live_display_layout(controller_window: HWND) {
    let windows = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => {
            controller.windows.values().copied().collect::<Vec<_>>()
        }
        _ => return,
    };
    if windows.is_empty() {
        return;
    }
    if windows.iter().any(|window| {
        matches!(
            state_mut(*window),
            Some(WindowState::Fence(FenceState {
                interaction: Some(_),
                ..
            }))
        )
    }) {
        schedule_display_layout_reconcile(controller_window);
        return;
    }
    let show_fence_titles = windows
        .iter()
        .find_map(|window| match state_mut(*window) {
            Some(WindowState::Fence(state)) => Some(state.preferences.show_fence_titles),
            _ => None,
        })
        .unwrap_or(true);
    let snapshots = windows
        .iter()
        .filter_map(|window| match state_mut(*window) {
            Some(WindowState::Fence(state)) => Some(state.snapshot.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let displays = available_display_anchors();
    let (mapped, events) = reconcile_display_layout(snapshots, &displays, show_fence_titles);
    if events.is_empty() {
        return;
    }
    let window_by_id = windows
        .iter()
        .filter_map(|window| match state_mut(*window) {
            Some(WindowState::Fence(state)) => Some((state.snapshot.id.clone(), *window)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    for snapshot in mapped {
        let Some(window) = window_by_id.get(&snapshot.id).copied() else {
            continue;
        };
        if let Some(WindowState::Fence(state)) = state_mut(window) {
            state.snapshot = snapshot;
        }
    }
    let should_place_layers = matches!(
        state_mut(controller_window),
        Some(WindowState::Controller(ControllerState {
            desktop_visible: true,
            hotkey_hidden: false,
            ..
        }))
    );
    for window in windows {
        let (snapshot, show_fence_titles) = match state_mut(window) {
            Some(WindowState::Fence(state)) => {
                (state.snapshot.clone(), state.preferences.show_fence_titles)
            }
            _ => continue,
        };
        let result = SetWindowPos(
            window,
            None,
            logical_i32(snapshot.x),
            window_y(snapshot.y, show_fence_titles),
            logical_i32(snapshot.width.max(f64::from(MIN_FENCE_WIDTH))),
            visible_height(&snapshot, show_fence_titles),
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(display_windows_error)
        .and_then(|_| render_fence_layered(window));
        if let Err(error) = result {
            emit_event(&HostEvent::Notification {
                message: format!("显示器变化后无法恢复盒子位置：{error}"),
            });
            continue;
        }
        clamp_fence_scroll(window);
        if should_place_layers {
            let _ = place_fence_layers(window);
        }
    }
    for event in events {
        emit_event(&event);
    }
}

unsafe fn apply_command(controller_window: HWND, command: HostCommand) {
    match command {
        HostCommand::Hello { protocol_version } => {
            log::info!(
                target: "ipc",
                "Hello received protocol={protocol_version} expected={PROTOCOL_VERSION}"
            );
            if protocol_version != PROTOCOL_VERSION {
                log::error!(
                    target: "ipc",
                    "protocol mismatch client={protocol_version} host={PROTOCOL_VERSION}"
                );
                emit_event(&HostEvent::Error {
                    message: format!(
                        "Desktop Host 协议不匹配：主程序 {protocol_version}，Host {PROTOCOL_VERSION}"
                    ),
                });
                shutdown(controller_window);
                return;
            }
            if let Some(WindowState::Controller(controller)) = state_mut(controller_window) {
                controller.handshake_complete = true;
            }
            emit_event(&HostEvent::Ready {
                protocol_version: PROTOCOL_VERSION,
            });
        }
        HostCommand::Sync {
            revision,
            fences,
            preferences,
            visible,
        } => {
            let requested_fence_count = fences.len();
            log::info!(
                target: "ipc",
                "Sync received revision={revision} fences={requested_fence_count} visible={visible}"
            );
            let handshake_complete = matches!(
                state_mut(controller_window),
                Some(WindowState::Controller(ControllerState {
                    handshake_complete: true,
                    ..
                }))
            );
            if !handshake_complete {
                log::error!(target: "ipc", "Sync rejected before Hello revision={revision}");
                emit_event(&HostEvent::Error {
                    message: "收到 Sync 前尚未完成 Hello 握手".into(),
                });
                return;
            }
            match sync_fences(controller_window, fences, preferences, visible) {
                Ok(fence_count) => {
                    log::info!(
                        target: "ipc",
                        "Sync completed revision={revision} fences={fence_count}"
                    );
                    emit_event(&HostEvent::Synced {
                        revision,
                        fence_count,
                    });
                }
                Err(message) => {
                    log::error!(
                        target: "ipc",
                        "Sync failed revision={revision} fences={requested_fence_count}: {message}"
                    );
                    emit_event(&HostEvent::Error { message });
                }
            }
        }
        HostCommand::RefreshFence { id } => {
            let window = match state_mut(controller_window) {
                Some(WindowState::Controller(controller)) => controller.windows.get(&id).copied(),
                _ => None,
            };
            if let Some(window) = window {
                refresh_fence_from_watch(window);
            }
        }
        HostCommand::SetHotkeyCapture { active } => {
            log::debug!(target: "hotkey", "hotkey capture active={active}");
            GHOST_KEYBOARD_HOOK_STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.capture_active = active;
                state.tracker.reset_pressed();
            });
        }
        HostCommand::Shutdown => {
            log::info!(target: "ipc", "Shutdown received");
            shutdown(controller_window);
        }
    }
}

unsafe fn sync_fences(
    controller_window: HWND,
    fences: Vec<HostFenceSnapshot>,
    preferences: HostPreferencesSnapshot,
    visible: bool,
) -> Result<usize, String> {
    let visible = configure_ghost_hotkey(controller_window, &preferences, visible)?;
    let displays = available_display_anchors();
    let (fences, layout_events) =
        reconcile_display_layout(fences, &displays, preferences.show_fence_titles);
    let desired: HashSet<&str> = fences.iter().map(|fence| fence.id.as_str()).collect();
    let removed: Vec<(String, HWND)> = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => controller
            .windows
            .iter()
            .filter(|(id, _)| !desired.contains(id.as_str()))
            .map(|(id, window)| (id.clone(), *window))
            .collect(),
        _ => return Err("Desktop Host 控制窗口状态无效".into()),
    };
    for (id, window) in removed {
        if let Some(WindowState::Controller(controller)) = state_mut(controller_window) {
            controller.windows.remove(&id);
        }
        let _ = DestroyWindow(window);
    }

    for snapshot in fences {
        let existing = match state_mut(controller_window) {
            Some(WindowState::Controller(controller)) => {
                controller.windows.get(&snapshot.id).copied()
            }
            _ => None,
        };
        let window = if let Some(window) = existing {
            if update_fence_window(window, snapshot, preferences.clone(), visible)? {
                place_fence_layers(window)?;
            }
            window
        } else {
            let window = create_fence_window(
                controller_window,
                snapshot.clone(),
                preferences.clone(),
                visible,
            )?;
            if let Some(WindowState::Controller(controller)) = state_mut(controller_window) {
                controller.windows.insert(snapshot.id, window);
            }
            window
        };
        let _ = InvalidateRect(Some(window), None, false);
    }

    // 先把布局变化写回主进程，再由随后的 Synced 确认本轮窗口同步完成。
    // 探针和主进程都允许 Sync 期间出现这些几何事件。
    for event in layout_events {
        emit_event(&event);
    }

    match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => Ok(controller.windows.len()),
        _ => Err("Desktop Host 控制窗口状态无效".into()),
    }
}

impl HotkeyChordTracker {
    fn set_chord(&mut self, chord: Option<HotkeyChord>) {
        self.chord = chord;
        self.reset_pressed();
    }

    fn reset_pressed(&mut self) {
        self.pressed.clear();
        self.suppressed.clear();
        self.buffered.clear();
        self.latched = false;
        self.cancelled = false;
    }

    #[cfg(test)]
    fn handle_key(&mut self, virtual_key: u32, key_down: bool) -> HotkeyHookResult {
        self.handle_event(ReplayKeyboardEvent::key(virtual_key, key_down))
    }

    fn handle_event(&mut self, event: ReplayKeyboardEvent) -> HotkeyHookResult {
        let virtual_key = normalized_hotkey_virtual_key(event.virtual_key);
        let Some(chord) = self.chord.clone() else {
            return HotkeyHookResult::default();
        };
        if !chord.keys.contains(&virtual_key) {
            if !self.buffered.is_empty() {
                return self.cancel_buffered(Some(event));
            }
            return HotkeyHookResult::default();
        }

        if event.key_down {
            let first_press = self.pressed.insert(virtual_key);
            if self.latched {
                let suppress = !is_hotkey_modifier(virtual_key);
                if suppress {
                    self.suppressed.insert(virtual_key);
                }
                return HotkeyHookResult {
                    suppress,
                    ..HotkeyHookResult::default()
                };
            }
            if self.cancelled {
                return HotkeyHookResult::default();
            }
            if !first_press {
                if !is_hotkey_modifier(virtual_key) && !self.buffered.is_empty() {
                    let result = self.cancel_buffered(Some(event));
                    // 回放序列的最后一个事件是当前自动重复的 key-down，后续真实
                    // key-up 需要继续传给前台应用，形成完整的一次按住操作。
                    self.suppressed.remove(&virtual_key);
                    return result;
                }
                return HotkeyHookResult::default();
            }
            if is_hotkey_modifier(virtual_key) {
                return HotkeyHookResult::default();
            }

            let modifiers_ready = chord
                .keys
                .iter()
                .filter(|key| is_hotkey_modifier(**key))
                .all(|key| self.pressed.contains(key));
            if !modifiers_ready {
                // 修饰键必须先按下。否则当前普通按键已经传给了前台程序，之后再
                // 补按修饰键不应突然触发 DCreel 快捷键。
                self.cancelled = true;
                return HotkeyHookResult::default();
            }

            self.suppressed.insert(virtual_key);
            let ordinary_count = chord
                .keys
                .iter()
                .filter(|key| !is_hotkey_modifier(**key))
                .count();
            if ordinary_count > 1 {
                self.buffered.push(event);
            }
            let trigger = chord.keys.iter().all(|key| self.pressed.contains(key));
            if trigger {
                self.latched = true;
                self.buffered.clear();
            }
            HotkeyHookResult {
                trigger,
                suppress: true,
                replay: Vec::new(),
            }
        } else {
            self.pressed.remove(&virtual_key);
            if self.latched {
                let suppress = self.suppressed.remove(&virtual_key);
                if self.pressed.is_empty() {
                    self.reset_gesture();
                }
                return HotkeyHookResult {
                    suppress,
                    ..HotkeyHookResult::default()
                };
            }

            if !self.buffered.is_empty() {
                let was_suppressed = self.suppressed.remove(&virtual_key);
                let trailing = (!was_suppressed).then_some(event);
                let result = self.cancel_buffered(trailing);
                if self.pressed.is_empty() {
                    self.reset_gesture();
                }
                return result;
            }

            if self.cancelled {
                let suppress = self.suppressed.remove(&virtual_key);
                if self.pressed.is_empty() {
                    self.reset_gesture();
                }
                return HotkeyHookResult {
                    suppress,
                    ..HotkeyHookResult::default()
                };
            }

            HotkeyHookResult::default()
        }
    }

    fn cancel_buffered(&mut self, trailing_event: Option<ReplayKeyboardEvent>) -> HotkeyHookResult {
        let mut replay =
            Vec::with_capacity(self.buffered.len() * 2 + usize::from(trailing_event.is_some()));
        for event in self.buffered.drain(..) {
            replay.push(event);
            replay.push(event.released());
        }
        replay.extend(trailing_event);
        self.cancelled = true;
        HotkeyHookResult {
            trigger: false,
            suppress: true,
            replay,
        }
    }

    fn reset_gesture(&mut self) {
        self.suppressed.clear();
        self.buffered.clear();
        self.latched = false;
        self.cancelled = false;
    }
}

unsafe fn replay_keyboard_events(events: &[ReplayKeyboardEvent]) {
    if events.is_empty() {
        return;
    }
    let inputs = events
        .iter()
        .map(|event| {
            let mut flags = KEYBD_EVENT_FLAGS::default();
            if event.extended {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if !event.key_down {
                flags |= KEYEVENTF_KEYUP;
            }
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(event.virtual_key as u16),
                        wScan: event.scan_code as u16,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: HOTKEY_REPLAY_EXTRA_INFO,
                    },
                },
            }
        })
        .collect::<Vec<_>>();
    let _ = SendInput(&inputs, size_of::<INPUT>() as i32);
}

unsafe extern "system" fn ghost_keyboard_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let message = wparam.0 as u32;
    let key_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
    if !key_down && !key_up {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let keyboard = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    if keyboard.dwExtraInfo == HOTKEY_REPLAY_EXTRA_INFO {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let event = ReplayKeyboardEvent {
        virtual_key: keyboard.vkCode,
        scan_code: keyboard.scanCode,
        key_down,
        extended: keyboard.flags.contains(LLKHF_EXTENDED),
    };
    let (controller_window, result) = GHOST_KEYBOARD_HOOK_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let controller_window = state.controller_window;
        let result = if state.capture_active {
            HotkeyHookResult::default()
        } else {
            state.tracker.handle_event(event)
        };
        (controller_window, result)
    });
    replay_keyboard_events(&result.replay);
    if result.trigger && !controller_window.0.is_null() {
        let _ = PostMessageW(
            Some(controller_window),
            WM_GHOST_HOTKEY_CHORD,
            WPARAM(0),
            LPARAM(0),
        );
    }
    if result.suppress {
        LRESULT(1)
    } else {
        CallNextHookEx(None, code, wparam, lparam)
    }
}

fn normalized_hotkey_virtual_key(virtual_key: u32) -> u32 {
    match virtual_key {
        0xa0 | 0xa1 => 0x10, // left/right Shift
        0xa2 | 0xa3 => 0x11, // left/right Ctrl
        0xa4 | 0xa5 => 0x12, // left/right Alt
        0x5c => 0x5b,        // right/left Windows key
        _ => virtual_key,
    }
}

fn is_hotkey_modifier(virtual_key: u32) -> bool {
    matches!(
        normalized_hotkey_virtual_key(virtual_key),
        0x10 | 0x11 | 0x12 | 0x5b
    )
}

fn parse_hotkey(value: &str) -> Result<HotkeyChord, String> {
    let mut keys = Vec::new();
    for token in value
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let virtual_key = match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => 0x11,
            "alt" => 0x12,
            "shift" => 0x10,
            "win" | "windows" | "meta" => 0x5b,
            _ => hotkey_virtual_key(token).ok_or_else(|| format!("无法识别快捷键按键：{token}"))?,
        };
        let virtual_key = normalized_hotkey_virtual_key(virtual_key);
        if keys.contains(&virtual_key) {
            return Err(format!("快捷键包含重复按键：{token}"));
        }
        keys.push(virtual_key);
    }
    if keys.is_empty() {
        return Err("快捷键不能为空".into());
    }
    if keys.len() > 8 {
        return Err("快捷键最多包含 8 个按键".into());
    }
    if keys.iter().all(|key| is_hotkey_modifier(*key)) {
        return Err("快捷键至少需要一个非修饰按键".into());
    }
    keys.sort_unstable();
    Ok(HotkeyChord { keys })
}

fn hotkey_virtual_key(token: &str) -> Option<u32> {
    let upper = token.to_ascii_uppercase();
    if upper.len() == 1 {
        let value = upper.as_bytes()[0];
        if value.is_ascii_alphanumeric() {
            return Some(u32::from(value));
        }
    }
    if let Some(number) = upper
        .strip_prefix('F')
        .and_then(|value| value.parse::<u32>().ok())
        && (1..=24).contains(&number)
    {
        return Some(0x70 + number - 1);
    }
    if let Some(number) = upper
        .strip_prefix("NUMPAD")
        .and_then(|value| value.parse::<u32>().ok())
        && number <= 9
    {
        return Some(0x60 + number);
    }
    match upper.as_str() {
        "BACKSPACE" => Some(0x08),
        "TAB" => Some(0x09),
        "ENTER" => Some(0x0d),
        "ESC" | "ESCAPE" => Some(0x1b),
        "SPACE" => Some(0x20),
        "PAGEUP" => Some(0x21),
        "PAGEDOWN" => Some(0x22),
        "END" => Some(0x23),
        "HOME" => Some(0x24),
        "LEFT" => Some(0x25),
        "UP" => Some(0x26),
        "RIGHT" => Some(0x27),
        "DOWN" => Some(0x28),
        "INSERT" => Some(0x2d),
        "DELETE" => Some(0x2e),
        "PRINTSCREEN" => Some(0x2c),
        "PAUSE" => Some(0x13),
        "CAPSLOCK" => Some(0x14),
        "SCROLLLOCK" => Some(0x91),
        "NUMPADMULTIPLY" => Some(0x6a),
        "NUMPADADD" => Some(0x6b),
        "NUMPADSUBTRACT" => Some(0x6d),
        "NUMPADDECIMAL" => Some(0x6e),
        "NUMPADDIVIDE" => Some(0x6f),
        "SEMICOLON" => Some(0xba),
        "PLUS" => Some(0xbb),
        "COMMA" => Some(0xbc),
        "MINUS" => Some(0xbd),
        "PERIOD" => Some(0xbe),
        "SLASH" => Some(0xbf),
        "BACKTICK" => Some(0xc0),
        "BRACKETLEFT" => Some(0xdb),
        "BACKSLASH" => Some(0xdc),
        "BRACKETRIGHT" => Some(0xdd),
        "QUOTE" => Some(0xde),
        _ => None,
    }
}

unsafe fn configure_ghost_hotkey(
    controller_window: HWND,
    preferences: &HostPreferencesSnapshot,
    visible: bool,
) -> Result<bool, String> {
    let desired =
        if preferences.ghost_mode && preferences.ghost_mode_trigger == GhostModeTrigger::Hotkey {
            Some(parse_hotkey(&preferences.ghost_hotkey)?)
        } else {
            None
        };
    let controller = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => controller,
        _ => return Err("Desktop Host 控制窗口状态无效".into()),
    };
    controller.desktop_visible = visible;
    let changed = controller.hotkey_chord != desired;
    if changed {
        controller.hotkey_chord = desired.clone();
        controller.hotkey_hidden = false;
    }
    if desired.is_none() {
        controller.hotkey_hidden = false;
    }
    if changed {
        GHOST_KEYBOARD_HOOK_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.controller_window = controller_window;
            state.tracker.set_chord(desired);
        });
    }
    Ok(controller.desktop_visible && !controller.hotkey_hidden)
}

unsafe fn toggle_hotkey_fences(controller_window: HWND) {
    let (windows, show) = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) if controller.hotkey_chord.is_some() => {
            controller.hotkey_hidden = !controller.hotkey_hidden;
            (
                controller.windows.values().copied().collect::<Vec<_>>(),
                controller.desktop_visible && !controller.hotkey_hidden,
            )
        }
        _ => return,
    };
    for window in windows {
        if let Some(WindowState::Fence(state)) = state_mut(window) {
            state.mouse_inside = false;
        }
        if show {
            // 隐藏期间像素缓冲区仍会由 Sync 保持最新，不需要在快捷键恢复时
            // 逐个同步重绘。先恢复桌面层级，再一次性显示，可避免可感知停顿。
            let _ = place_fence_layers(window);
        }
        let _ = ShowWindow(window, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

unsafe fn create_fence_window(
    controller_window: HWND,
    snapshot: HostFenceSnapshot,
    preferences: HostPreferencesSnapshot,
    visible: bool,
) -> Result<HWND, String> {
    let (instance, class_name) = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => {
            (controller.instance, controller.class_name.clone())
        }
        _ => return Err("Desktop Host 控制窗口状态无效".into()),
    };
    let title: Vec<u16> = snapshot.title.encode_utf16().chain(Some(0)).collect();
    let x = logical_i32(snapshot.x);
    let y = window_y(snapshot.y, preferences.show_fence_titles);
    let width = logical_i32(snapshot.width.max(244.0));
    let height = visible_height(&snapshot, preferences.show_fence_titles);
    let state = Box::new(WindowState::Fence(FenceState {
        controller_window,
        items: list_folder(&snapshot.directory, preferences.show_hidden_files),
        snapshot,
        preferences,
        visuals: HashMap::new(),
        interaction: None,
        item_drag: None,
        scroll_row: 0,
        wheel_delta_remainder: 0,
        last_folder_refresh: Instant::now(),
        last_layer_refresh: Instant::now(),
        drop_target: None,
        active_shell_menu: None,
        selected_paths: HashSet::new(),
        selection_anchor: None,
        focused_path: None,
        marquee: None,
        mouse_inside: false,
        undo_geometry: None,
        foreground_until: None,
        visible,
    }));
    let raw_state = Box::into_raw(state);
    let window = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_LAYERED,
        PCWSTR(class_name.as_ptr()),
        PCWSTR(title.as_ptr()),
        WS_POPUP | WS_VISIBLE,
        x,
        y,
        width,
        height,
        None,
        None,
        Some(instance),
        Some(raw_state.cast::<c_void>()),
    )
    .map_err(display_windows_error)?;
    if let Err(error) = register_fence_drop_target(window) {
        let _ = DestroyWindow(window);
        return Err(error);
    }
    apply_fence_window(window, visible)?;
    place_fence_layers(window)?;
    if SetTimer(Some(window), FENCE_TIMER, FENCE_TIMER_INTERVAL_MS, None) == 0 {
        let _ = DestroyWindow(window);
        return Err(display_windows_error(Error::from_thread()));
    }
    let _ = UpdateWindow(window);
    Ok(window)
}

unsafe fn register_fence_drop_target(window: HWND) -> Result<(), String> {
    let target: IDropTarget = FenceDropTarget::new(window).into();
    RegisterDragDrop(window, &target).map_err(display_windows_error)?;
    match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            state.drop_target = Some(target);
            Ok(())
        }
        _ => {
            let _ = RevokeDragDrop(window);
            Err("Desktop Host 盒子窗口状态无效".into())
        }
    }
}

unsafe fn update_fence_window(
    window: HWND,
    mut snapshot: HostFenceSnapshot,
    preferences: HostPreferencesSnapshot,
    visible: bool,
) -> Result<bool, String> {
    match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let unchanged = state.snapshot == snapshot
                && state.preferences == preferences
                && state.visible == visible;
            if unchanged {
                return Ok(false);
            }
            if state.snapshot.directory != snapshot.directory
                || state.preferences.show_hidden_files != preferences.show_hidden_files
            {
                state.items = list_folder(&snapshot.directory, preferences.show_hidden_files);
                state.visuals.clear();
                state.scroll_row = 0;
                state.wheel_delta_remainder = 0;
                state.selected_paths.clear();
                state.selection_anchor = None;
                state.focused_path = None;
                state.marquee = None;
                state.last_folder_refresh = Instant::now();
            }
            if state.preferences.icon_size != preferences.icon_size {
                state.visuals.clear();
            }
            // Watchdog 可能在用户尚未松开鼠标时发送旧快照。交互过程中以
            // Host 的实时窗口矩形为准，防止盒子被同步弹回拖动前的位置。
            if state.interaction.is_some() {
                snapshot.x = state.snapshot.x;
                snapshot.y = state.snapshot.y;
                snapshot.width = state.snapshot.width;
                snapshot.height = state.snapshot.height;
            }
            state.snapshot = snapshot;
            state.preferences = preferences;
            state.visible = visible;
        }
        _ => return Err("Desktop Host 盒子窗口状态无效".into()),
    }
    apply_fence_window(window, visible)?;
    Ok(true)
}

unsafe fn apply_fence_window(window: HWND, visible: bool) -> Result<(), String> {
    let (snapshot, show_fence_titles) = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            if !visible {
                state.mouse_inside = false;
            }
            (state.snapshot.clone(), state.preferences.show_fence_titles)
        }
        _ => return Err("Desktop Host 盒子窗口状态无效".into()),
    };
    let title: Vec<u16> = snapshot.title.encode_utf16().chain(Some(0)).collect();
    SetWindowTextW(window, PCWSTR(title.as_ptr())).map_err(display_windows_error)?;
    SetWindowPos(
        window,
        None,
        logical_i32(snapshot.x),
        window_y(snapshot.y, show_fence_titles),
        logical_i32(snapshot.width.max(244.0)),
        visible_height(&snapshot, show_fence_titles),
        SWP_NOACTIVATE | SWP_NOZORDER,
    )
    .map_err(display_windows_error)?;
    render_fence_layered(window)?;
    clamp_fence_scroll(window);
    configure_transparent_ghost_hover_poll(window)?;
    let _ = ShowWindow(window, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
    Ok(())
}

fn panel_background_alpha(opacity: f64) -> u8 {
    // alpha=0 的 Layered Window 像素会直接穿透鼠标。用 1/255 保留空白背景的
    // 拖动和右键命中，视觉上仍等同完全透明。
    1u8.saturating_add(opacity_alpha(opacity).saturating_sub(1))
}

fn opacity_alpha(opacity: f64) -> u8 {
    let opacity = if opacity.is_finite() {
        opacity.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (opacity * 255.0).round() as u8
}

fn ghost_surface_alpha(ghost_mode: bool, mouse_inside: bool, ghost_opacity: f64) -> u8 {
    if ghost_mode && !mouse_inside {
        let opacity = if ghost_opacity.is_finite() {
            ghost_opacity.clamp(0.0, 1.0)
        } else {
            0.2
        };
        (opacity * 255.0).round() as u8
    } else {
        255
    }
}

fn needs_transparent_ghost_hover_poll(preferences: &HostPreferencesSnapshot) -> bool {
    preferences.ghost_mode
        && preferences.ghost_mode_trigger == GhostModeTrigger::Automatic
        && ghost_surface_alpha(true, false, preferences.ghost_opacity) == 0
}

unsafe fn configure_transparent_ghost_hover_poll(window: HWND) -> Result<(), String> {
    let enabled = match state_mut(window) {
        Some(WindowState::Fence(state)) => needs_transparent_ghost_hover_poll(&state.preferences),
        _ => return Err("Desktop Host 盒子窗口状态无效".into()),
    };
    if enabled {
        if SetTimer(
            Some(window),
            TRANSPARENT_GHOST_HOVER_TIMER,
            TRANSPARENT_GHOST_HOVER_INTERVAL_MS,
            None,
        ) == 0
        {
            return Err(display_windows_error(Error::from_thread()));
        }
    } else {
        let _ = KillTimer(Some(window), TRANSPARENT_GHOST_HOVER_TIMER);
    }
    Ok(())
}

unsafe fn poll_transparent_ghost_hover(window: HWND) {
    let (enabled, previous) = match state_mut(window) {
        Some(WindowState::Fence(state)) => (
            needs_transparent_ghost_hover_poll(&state.preferences),
            state.mouse_inside,
        ),
        _ => return,
    };
    if !enabled {
        let _ = KillTimer(Some(window), TRANSPARENT_GHOST_HOVER_TIMER);
        return;
    }

    let mut cursor = POINT::default();
    let mut bounds = RECT::default();
    if GetCursorPos(&mut cursor).is_err() || GetWindowRect(window, &mut bounds).is_err() {
        return;
    }
    let inside = point_in_rect(cursor, &bounds);
    if inside == previous {
        return;
    }
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.mouse_inside = inside;
    }
    redraw_fence(window);
}

unsafe fn enter_fence_hover(window: HWND) -> bool {
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state)) if !state.mouse_inside => {
            state.mouse_inside = true;
            true
        }
        Some(WindowState::Fence(_)) => false,
        _ => return false,
    };
    if !changed {
        return false;
    }
    let mut tracking = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: window,
        dwHoverTime: 0,
    };
    if TrackMouseEvent(&mut tracking).is_err()
        && let Some(WindowState::Fence(state)) = state_mut(window)
    {
        state.mouse_inside = false;
    }
    redraw_fence(window);
    true
}

unsafe fn leave_fence_hover(window: HWND) {
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let changed = state.mouse_inside;
            state.mouse_inside = false;
            changed
        }
        _ => false,
    };
    if changed {
        redraw_fence(window);
    }
}

unsafe fn redraw_fence(window: HWND) {
    let _ = InvalidateRect(Some(window), None, false);
    let _ = UpdateWindow(window);
}

unsafe fn update_fence_cursor(window: HWND) -> bool {
    let mut cursor_position = POINT::default();
    if GetCursorPos(&mut cursor_position).is_err() {
        return false;
    }
    let mode = match state_mut(window) {
        Some(WindowState::Fence(state)) => state
            .interaction
            .as_ref()
            .map(|interaction| interaction.mode),
        _ => None,
    }
    .or_else(|| interaction_mode_at(window, cursor_position));
    let Some(mode) = mode else {
        return false;
    };
    let cursor_id = match mode {
        InteractionMode::Move => IDC_SIZEALL,
        InteractionMode::Resize(edges)
            if (edges.left && edges.top) || (edges.right && edges.bottom) =>
        {
            IDC_SIZENWSE
        }
        InteractionMode::Resize(edges)
            if (edges.right && edges.top) || (edges.left && edges.bottom) =>
        {
            IDC_SIZENESW
        }
        InteractionMode::Resize(edges) if edges.left || edges.right => IDC_SIZEWE,
        InteractionMode::Resize(_) => IDC_SIZENS,
    };
    match LoadCursorW(None, cursor_id) {
        Ok(cursor) => {
            let _ = SetCursor(Some(cursor));
            true
        }
        Err(_) => false,
    }
}

unsafe fn begin_fence_interaction(window: HWND) -> bool {
    let mut cursor = POINT::default();
    if GetCursorPos(&mut cursor).is_err() {
        return false;
    }
    let Some(mode) = interaction_mode_at(window, cursor) else {
        return false;
    };
    let mut rect = RECT::default();
    if GetWindowRect(window, &mut rect).is_err() {
        return false;
    }
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        let start_geometry = FenceGeometry::from_snapshot(&state.snapshot);
        state.interaction = Some(WindowInteraction {
            mode,
            start_cursor: cursor,
            start_rect: rect,
            start_geometry,
            changed: false,
        });
        let _ = SetCapture(window);
        true
    } else {
        false
    }
}

unsafe fn update_fence_interaction(window: HWND) -> bool {
    let (mode, start_cursor, start_rect, show_fence_titles) = match state_mut(window) {
        Some(WindowState::Fence(FenceState {
            interaction: Some(interaction),
            preferences,
            ..
        })) => (
            interaction.mode,
            interaction.start_cursor,
            interaction.start_rect,
            preferences.show_fence_titles,
        ),
        _ => return false,
    };
    let mut cursor = POINT::default();
    if GetCursorPos(&mut cursor).is_err() {
        return true;
    }
    let hidden_offset = hidden_title_offset(show_fence_titles);
    let rect = interaction_rect(
        mode,
        start_cursor,
        start_rect,
        cursor,
        MIN_FENCE_HEIGHT.saturating_sub(hidden_offset),
        MAX_FENCE_HEIGHT.saturating_sub(hidden_offset),
    );
    let work_area = display_anchor_for_rect(&rect).map(|anchor| display_work_rect(&anchor));
    let rect = work_area
        .as_ref()
        .map(|work| {
            snap_and_constrain_to_work_area(
                mode,
                rect,
                work,
                MIN_FENCE_HEIGHT.saturating_sub(hidden_offset),
                MAX_FENCE_HEIGHT.saturating_sub(hidden_offset),
            )
        })
        .unwrap_or(rect);
    let siblings = sibling_window_rects(window);
    let rect = snap_interaction_rect(
        mode,
        rect,
        &siblings,
        MIN_FENCE_HEIGHT.saturating_sub(hidden_offset),
        MAX_FENCE_HEIGHT.saturating_sub(hidden_offset),
    );
    let rect = prevent_sibling_overlap(
        mode,
        start_rect,
        rect,
        &siblings,
        MIN_FENCE_HEIGHT.saturating_sub(hidden_offset),
        MAX_FENCE_HEIGHT.saturating_sub(hidden_offset),
    );
    let rect = work_area
        .as_ref()
        .map(|work| {
            snap_and_constrain_to_work_area(
                mode,
                rect,
                work,
                MIN_FENCE_HEIGHT.saturating_sub(hidden_offset),
                MAX_FENCE_HEIGHT.saturating_sub(hidden_offset),
            )
        })
        .unwrap_or(rect);
    if SetWindowPos(
        window,
        None,
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
        SWP_NOACTIVATE | SWP_NOZORDER,
    )
    .is_err()
    {
        return true;
    }
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.snapshot.x = f64::from(rect.left);
        state.snapshot.y = f64::from(rect.top.saturating_sub(hidden_offset));
        state.snapshot.width = f64::from(rect.right - rect.left);
        if !effectively_collapsed(&state.snapshot, show_fence_titles) {
            state.snapshot.height =
                f64::from((rect.bottom - rect.top).saturating_add(hidden_offset));
        }
        if let Some(interaction) = state.interaction.as_mut() {
            interaction.changed |= !rects_equal(&rect, &interaction.start_rect);
        }
    }
    clamp_fence_scroll(window);
    if matches!(mode, InteractionMode::Resize(_)) {
        let _ = render_fence_layered(window);
    }
    true
}

unsafe fn finish_fence_interaction(window: HWND) -> bool {
    let mut window_rect = RECT::default();
    let display_anchor = GetWindowRect(window, &mut window_rect)
        .ok()
        .and_then(|_| display_anchor_for_rect(&window_rect));
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let Some(interaction) = state.interaction.take() else {
                return false;
            };
            interaction.changed.then(|| {
                state.undo_geometry = Some(interaction.start_geometry);
                state.snapshot.display_anchor = display_anchor.clone();
                state.snapshot.placement = None;
                (state.controller_window, state.snapshot.id.clone())
            })
        }
        _ => return false,
    };
    if let Some((controller_window, id)) = changed {
        refresh_live_layout_placements(controller_window, Some(&id));
    }
    true
}

unsafe fn refresh_live_layout_placements(controller_window: HWND, force_id: Option<&str>) {
    let windows = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => {
            controller.windows.values().copied().collect::<Vec<_>>()
        }
        _ => return,
    };
    let show_fence_titles = windows
        .iter()
        .find_map(|window| match state_mut(*window) {
            Some(WindowState::Fence(state)) => Some(state.preferences.show_fence_titles),
            _ => None,
        })
        .unwrap_or(true);
    let mut snapshots = windows
        .iter()
        .filter_map(|window| match state_mut(*window) {
            Some(WindowState::Fence(state)) => Some(state.snapshot.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let previous = snapshots
        .iter()
        .map(|snapshot| (snapshot.id.clone(), snapshot.placement.clone()))
        .collect::<HashMap<_, _>>();
    assign_layout_placements(&mut snapshots, show_fence_titles);
    let window_by_id = windows
        .iter()
        .filter_map(|window| match state_mut(*window) {
            Some(WindowState::Fence(state)) => Some((state.snapshot.id.clone(), *window)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    for snapshot in snapshots {
        if let Some(window) = window_by_id.get(&snapshot.id).copied()
            && let Some(WindowState::Fence(state)) = state_mut(window)
        {
            state.snapshot.placement = snapshot.placement.clone();
        }
        let placement_changed = previous
            .get(&snapshot.id)
            .is_none_or(|placement| *placement != snapshot.placement);
        if placement_changed || force_id == Some(snapshot.id.as_str()) {
            emit_event(&geometry_changed_event(&snapshot));
        }
    }
}

fn selection_modifiers(buttons: WPARAM) -> SelectionModifiers {
    SelectionModifiers {
        control: buttons.0 & MK_CONTROL.0 as usize != 0,
        shift: buttons.0 & MK_SHIFT.0 as usize != 0,
    }
}

fn apply_item_selection(
    items: &[FolderItem],
    selected_paths: &mut HashSet<PathBuf>,
    selection_anchor: &mut Option<PathBuf>,
    index: usize,
    modifiers: SelectionModifiers,
) -> bool {
    let Some(clicked) = items.get(index).map(|item| item.path.clone()) else {
        return false;
    };
    if modifiers.shift {
        let anchor_index = selection_anchor
            .as_ref()
            .and_then(|anchor| items.iter().position(|item| item.path == *anchor))
            .unwrap_or(index);
        if !modifiers.control {
            selected_paths.clear();
        }
        let first = anchor_index.min(index);
        let last = anchor_index.max(index);
        selected_paths.extend(items[first..=last].iter().map(|item| item.path.clone()));
        if selection_anchor.is_none() {
            *selection_anchor = Some(clicked);
        }
        return false;
    }
    if modifiers.control {
        if !selected_paths.insert(clicked.clone()) {
            selected_paths.remove(&clicked);
        }
        *selection_anchor = Some(clicked);
        return false;
    }
    let collapse_on_release = selected_paths.contains(&clicked) && selected_paths.len() > 1;
    if !selected_paths.contains(&clicked) {
        selected_paths.clear();
        selected_paths.insert(clicked.clone());
    }
    *selection_anchor = Some(clicked);
    collapse_on_release
}

fn navigation_target(
    current: Option<usize>,
    item_count: usize,
    columns: usize,
    direction: NavigationDirection,
) -> Option<usize> {
    if item_count == 0 || columns == 0 {
        return None;
    }
    let Some(current) = current.filter(|index| *index < item_count) else {
        return Some(0);
    };
    match direction {
        NavigationDirection::Left => Some(current.saturating_sub(1)),
        NavigationDirection::Right => Some(current.saturating_add(1).min(item_count - 1)),
        NavigationDirection::Up => Some(current.checked_sub(columns).unwrap_or(current)),
        NavigationDirection::Down => Some(
            current
                .checked_add(columns)
                .filter(|target| *target < item_count)
                .unwrap_or(current),
        ),
    }
}

fn scroll_row_for_item(
    current_scroll_row: usize,
    item_index: usize,
    metrics: GridMetrics,
) -> usize {
    if metrics.columns == 0 || metrics.visible_rows == 0 {
        return current_scroll_row;
    }
    let item_row = item_index / metrics.columns;
    if item_row < current_scroll_row {
        item_row
    } else if item_row >= current_scroll_row.saturating_add(metrics.visible_rows) {
        item_row
            .saturating_add(1)
            .saturating_sub(metrics.visible_rows)
            .min(metrics.max_scroll_row)
    } else {
        current_scroll_row.min(metrics.max_scroll_row)
    }
}

fn selected_paths_in_item_order(
    items: &[FolderItem],
    selected_paths: &HashSet<PathBuf>,
    focused_path: Option<&Path>,
) -> Vec<PathBuf> {
    let paths: Vec<PathBuf> = items
        .iter()
        .filter(|item| selected_paths.contains(&item.path))
        .map(|item| item.path.clone())
        .collect();
    if paths.is_empty() {
        focused_path
            .filter(|focused| items.iter().any(|item| item.path == *focused))
            .map(|focused| vec![focused.to_path_buf()])
            .unwrap_or_default()
    } else {
        paths
    }
}

unsafe fn handle_fence_keydown(window: HWND, key: WPARAM) -> bool {
    let key = key.0;
    let control = GetKeyState(i32::from(VK_CONTROL.0)) < 0;
    let shift = GetKeyState(i32::from(VK_SHIFT.0)) < 0;

    if key == usize::from(VK_F5.0) {
        refresh_fence(window, true);
        return true;
    }

    if control && shift && key == usize::from(VK_N.0) {
        if let Err(error) = create_folder_in_fence(window) {
            emit_event(&HostEvent::Notification {
                message: format!("无法新建文件夹：{error}"),
            });
        }
        return true;
    }

    if key == usize::from(VK_APPS.0) || (shift && key == usize::from(VK_F10.0)) {
        return show_keyboard_context_menu(window);
    }

    if key == usize::from(VK_ESCAPE.0) {
        let changed = match state_mut(window) {
            Some(WindowState::Fence(state)) => {
                let changed = !state.selected_paths.is_empty()
                    || state.selection_anchor.is_some()
                    || state.focused_path.is_some()
                    || state.item_drag.is_some()
                    || state.marquee.is_some();
                state.selected_paths.clear();
                state.selection_anchor = None;
                state.focused_path = None;
                state.item_drag = None;
                state.marquee = None;
                changed
            }
            _ => return false,
        };
        if GetCapture() == window {
            let _ = ReleaseCapture();
        }
        if changed {
            let _ = InvalidateRect(Some(window), None, false);
        }
        return true;
    }

    if control && key == usize::from(VK_A.0) {
        let changed = match state_mut(window) {
            Some(WindowState::Fence(state))
                if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) =>
            {
                let previous = state.selected_paths.clone();
                let previous_focus = state.focused_path.clone();
                let previous_anchor = state.selection_anchor.clone();
                state.selected_paths = state.items.iter().map(|item| item.path.clone()).collect();
                let focus = state
                    .focused_path
                    .as_ref()
                    .filter(|path| state.selected_paths.contains(*path))
                    .cloned()
                    .or_else(|| state.items.first().map(|item| item.path.clone()));
                state.focused_path = focus.clone();
                state.selection_anchor = focus;
                previous != state.selected_paths
                    || previous_focus != state.focused_path
                    || previous_anchor != state.selection_anchor
            }
            Some(WindowState::Fence(_)) => false,
            _ => return false,
        };
        if changed {
            let _ = InvalidateRect(Some(window), None, false);
        }
        return true;
    }

    let direction = if key == usize::from(VK_LEFT.0) {
        Some(NavigationDirection::Left)
    } else if key == usize::from(VK_RIGHT.0) {
        Some(NavigationDirection::Right)
    } else if key == usize::from(VK_UP.0) {
        Some(NavigationDirection::Up)
    } else if key == usize::from(VK_DOWN.0) {
        Some(NavigationDirection::Down)
    } else {
        None
    };
    if let Some(direction) = direction {
        let mut client = RECT::default();
        if GetClientRect(window, &mut client).is_err() {
            return true;
        }
        let changed = match state_mut(window) {
            Some(WindowState::Fence(state))
                if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) =>
            {
                let Some(metrics) = grid_metrics(
                    &client,
                    content_top(state.preferences.show_fence_titles),
                    state.preferences.icon_size,
                    state.items.len(),
                ) else {
                    return true;
                };
                let current = state
                    .focused_path
                    .as_ref()
                    .and_then(|path| state.items.iter().position(|item| item.path == *path))
                    .or_else(|| {
                        state
                            .items
                            .iter()
                            .position(|item| state.selected_paths.contains(&item.path))
                    });
                let Some(target) =
                    navigation_target(current, state.items.len(), metrics.columns, direction)
                else {
                    return true;
                };
                let previous_selection = state.selected_paths.clone();
                let previous_focus = state.focused_path.clone();
                let previous_scroll = state.scroll_row;
                if shift {
                    if state.selection_anchor.is_none() {
                        state.selection_anchor = current
                            .and_then(|index| state.items.get(index))
                            .map(|item| item.path.clone())
                            .or_else(|| state.items.get(target).map(|item| item.path.clone()));
                    }
                    apply_item_selection(
                        &state.items,
                        &mut state.selected_paths,
                        &mut state.selection_anchor,
                        target,
                        SelectionModifiers {
                            control,
                            shift: true,
                        },
                    );
                } else if let Some(item) = state.items.get(target) {
                    state.selected_paths.clear();
                    state.selected_paths.insert(item.path.clone());
                    state.selection_anchor = Some(item.path.clone());
                }
                state.focused_path = state.items.get(target).map(|item| item.path.clone());
                state.scroll_row = scroll_row_for_item(state.scroll_row, target, metrics);
                previous_selection != state.selected_paths
                    || previous_focus != state.focused_path
                    || previous_scroll != state.scroll_row
            }
            Some(WindowState::Fence(_)) => false,
            _ => return false,
        };
        if changed {
            let _ = InvalidateRect(Some(window), None, false);
        }
        return true;
    }

    if key == usize::from(VK_RETURN.0) {
        let paths = match state_mut(window) {
            Some(WindowState::Fence(state)) => selected_paths_in_item_order(
                &state.items,
                &state.selected_paths,
                state.focused_path.as_deref(),
            ),
            _ => return false,
        };
        if paths.len() > MAX_KEYBOARD_OPEN_ITEMS {
            emit_event(&HostEvent::Notification {
                message: format!("一次最多打开 {MAX_KEYBOARD_OPEN_ITEMS} 个项目，请缩小选择范围"),
            });
            return true;
        }
        for path in paths {
            if let Err(error) = open_shell_path(window, &path) {
                emit_event(&HostEvent::Notification {
                    message: format!("无法打开 {}：{error}", path.display()),
                });
            }
        }
        return true;
    }

    if key == usize::from(VK_DELETE.0) {
        let paths = match state_mut(window) {
            Some(WindowState::Fence(state)) => selected_paths_in_item_order(
                &state.items,
                &state.selected_paths,
                state.focused_path.as_deref(),
            ),
            _ => return false,
        };
        if !paths.is_empty()
            && let Err(error) = invoke_shell_verb(window, &paths, "delete")
        {
            emit_event(&HostEvent::Notification {
                message: format!("无法删除所选项目：{error}"),
            });
        }
        return true;
    }

    if key == usize::from(VK_F2.0) {
        if let Err(error) = rename_selected_item(window) {
            emit_event(&HostEvent::Notification {
                message: format!("无法修改项目名称：{error}"),
            });
        }
        return true;
    }

    false
}

fn validate_windows_item_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("项目名称不能为空".into());
    }
    if name == "." || name == ".." {
        return Err("项目名称不能是 . 或 ..".into());
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err("项目名称不能以空格或句点结尾".into());
    }
    if name.chars().any(|character| {
        character < ' '
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
    }) {
        return Err("项目名称包含 Windows 不允许的字符".into());
    }
    let device_name = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    let reserved = matches!(device_name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device_name.len() == 4
            && (device_name.starts_with("COM") || device_name.starts_with("LPT"))
            && matches!(device_name.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        return Err("项目名称是 Windows 保留设备名".into());
    }
    Ok(())
}

fn available_new_folder_name(directory: &Path) -> Result<String, String> {
    for index in 1..=10_000 {
        let name = if index == 1 {
            "新建文件夹".to_string()
        } else {
            format!("新建文件夹 ({index})")
        };
        if !directory.join(&name).exists() {
            return Ok(name);
        }
    }
    Err("无法生成不重复的文件夹名称".into())
}

unsafe fn create_folder_in_fence(window: HWND) -> Result<(), String> {
    let directory = match state_mut(window) {
        Some(WindowState::Fence(state)) => state.snapshot.directory.clone(),
        _ => return Err("Desktop Host 盒子窗口状态无效".into()),
    };
    if !directory.is_dir() {
        return Err(format!("盒子目录已经不存在：{}", directory.display()));
    }
    let suggested = available_new_folder_name(&directory)?;
    let Some(name) = prompt_rename(
        window,
        "新建文件夹",
        "文件夹名称：",
        &suggested,
        "文件夹名称不能为空",
    )?
    else {
        return Ok(());
    };
    validate_windows_item_name(&name)?;
    let target = directory.join(&name);
    if target.exists() {
        return Err(format!("同名项目已经存在：{}", target.display()));
    }
    fs::create_dir(&target).map_err(|error| error.to_string())?;
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.selected_paths.clear();
        state.selected_paths.insert(target.clone());
        state.selection_anchor = Some(target.clone());
        state.focused_path = Some(target);
    }
    refresh_fence(window, true);
    Ok(())
}

unsafe fn undo_fence_geometry(window: HWND) -> Result<bool, String> {
    let (geometry, collapsed, show_fence_titles, id, controller_window) = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let Some(geometry) = state.undo_geometry.take() else {
                return Ok(false);
            };
            (
                geometry,
                state.snapshot.collapsed,
                state.preferences.show_fence_titles,
                state.snapshot.id.clone(),
                state.controller_window,
            )
        }
        _ => return Err("Desktop Host 盒子窗口状态无效".into()),
    };
    let height = if collapsed && show_fence_titles {
        HEADER_HEIGHT
    } else {
        logical_i32(geometry.height.max(f64::from(MIN_FENCE_HEIGHT)))
            .saturating_sub(hidden_title_offset(show_fence_titles))
            .max(1)
    };
    if let Err(error) = SetWindowPos(
        window,
        None,
        logical_i32(geometry.x),
        window_y(geometry.y, show_fence_titles),
        logical_i32(geometry.width.max(f64::from(MIN_FENCE_WIDTH))),
        height,
        SWP_NOACTIVATE | SWP_NOZORDER,
    ) {
        if let Some(WindowState::Fence(state)) = state_mut(window) {
            state.undo_geometry = Some(geometry);
        }
        return Err(display_windows_error(error));
    }
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.snapshot.x = geometry.x;
        state.snapshot.y = geometry.y;
        state.snapshot.width = geometry.width;
        state.snapshot.height = geometry.height;
        state.snapshot.placement = None;
    }
    let mut window_rect = RECT::default();
    let display_anchor = GetWindowRect(window, &mut window_rect)
        .ok()
        .and_then(|_| display_anchor_for_rect(&window_rect));
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.snapshot.display_anchor = display_anchor.clone();
    }
    clamp_fence_scroll(window);
    let _ = InvalidateRect(Some(window), None, false);
    refresh_live_layout_placements(controller_window, Some(&id));
    Ok(true)
}

unsafe fn rename_selected_item(window: HWND) -> Result<(), String> {
    let paths = match state_mut(window) {
        Some(WindowState::Fence(state)) => selected_paths_in_item_order(
            &state.items,
            &state.selected_paths,
            state.focused_path.as_deref(),
        ),
        _ => return Err("Desktop Host 盒子窗口状态无效".into()),
    };
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() != 1 {
        return Err("F2 一次只能修改一个项目，请先只选择一个项目".into());
    }
    let source = &paths[0];
    let current_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "项目名称不是有效的 Unicode 文本".to_string())?;
    let Some(new_name) = prompt_rename(
        window,
        "修改项目名称",
        "项目名称：",
        current_name,
        "项目名称不能为空",
    )?
    else {
        return Ok(());
    };
    if new_name == current_name {
        return Ok(());
    }
    validate_windows_item_name(&new_name)?;
    let parent = source
        .parent()
        .ok_or_else(|| "无法确定项目所在文件夹".to_string())?;
    let target = parent.join(&new_name);
    if target.exists() && new_name.to_lowercase() != current_name.to_lowercase() {
        return Err(format!("同名项目已经存在：{}", target.display()));
    }
    fs::rename(source, &target).map_err(|error| error.to_string())?;
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.selected_paths.clear();
        state.selected_paths.insert(target.clone());
        state.selection_anchor = Some(target.clone());
        state.focused_path = Some(target);
    }
    refresh_fence(window, true);
    Ok(())
}

unsafe fn begin_item_drag(window: HWND, point: POINT, buttons: WPARAM) -> bool {
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return false;
    }
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state))
            if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) =>
        {
            let Some(index) = item_index_at(
                &client,
                content_top(state.preferences.show_fence_titles),
                state.preferences.icon_size,
                state.items.len(),
                state.scroll_row,
                point,
            ) else {
                return false;
            };
            let previous = state.selected_paths.clone();
            let collapse_on_release = apply_item_selection(
                &state.items,
                &mut state.selected_paths,
                &mut state.selection_anchor,
                index,
                selection_modifiers(buttons),
            );
            state.focused_path = state.items.get(index).map(|item| item.path.clone());
            state.item_drag = state.items.get(index).map(|item| ItemDragCandidate {
                path: item.path.clone(),
                start: point,
                collapse_on_release,
            });
            previous != state.selected_paths
        }
        _ => return false,
    };
    if !matches!(
        state_mut(window),
        Some(WindowState::Fence(FenceState {
            item_drag: Some(_),
            ..
        }))
    ) {
        return false;
    }
    if changed {
        redraw_fence(window);
    }
    let _ = SetFocus(Some(window));
    let _ = SetCapture(window);
    true
}

unsafe fn update_item_drag(window: HWND, point: POINT, buttons: WPARAM) -> bool {
    let candidate = match state_mut(window) {
        Some(WindowState::Fence(state)) => state.item_drag.as_ref(),
        _ => return false,
    };
    let Some(candidate) = candidate else {
        return false;
    };
    if buttons.0 & MK_LBUTTON.0 as usize == 0 {
        return cancel_item_drag(window);
    }
    let crossed_threshold = item_drag_crossed_threshold(candidate.start, point);
    if !crossed_threshold {
        return true;
    }
    let paths = match state_mut(window) {
        Some(WindowState::Fence(state)) => state.item_drag.take().map(|candidate| {
            selected_paths_for_drag(&state.items, &state.selected_paths, &candidate.path)
        }),
        _ => None,
    };
    if GetCapture() == window {
        let _ = ReleaseCapture();
    }
    if let Some(paths) = paths
        && let Err(error) = start_file_drag(window, paths)
    {
        emit_event(&HostEvent::Notification {
            message: format!("无法拖出项目：{error}"),
        });
    }
    true
}

fn item_drag_crossed_threshold(start: POINT, current: POINT) -> bool {
    current.x.abs_diff(start.x) >= ITEM_DRAG_THRESHOLD as u32
        || current.y.abs_diff(start.y) >= ITEM_DRAG_THRESHOLD as u32
}

unsafe fn cancel_item_drag(window: HWND) -> bool {
    match state_mut(window) {
        Some(WindowState::Fence(state)) => state.item_drag.take().is_some(),
        _ => false,
    }
}

unsafe fn finish_item_pointer(window: HWND) -> bool {
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let Some(candidate) = state.item_drag.take() else {
                return false;
            };
            if candidate.collapse_on_release
                && state.items.iter().any(|item| item.path == candidate.path)
            {
                state.selected_paths.clear();
                state.selected_paths.insert(candidate.path.clone());
                state.selection_anchor = Some(candidate.path);
                state.focused_path = state.selection_anchor.clone();
                true
            } else {
                false
            }
        }
        _ => return false,
    };
    if changed {
        let _ = InvalidateRect(Some(window), None, false);
    }
    true
}

fn selected_paths_for_drag(
    items: &[FolderItem],
    selected_paths: &HashSet<PathBuf>,
    clicked_path: &Path,
) -> Vec<PathBuf> {
    if !selected_paths.contains(clicked_path) {
        return vec![clicked_path.to_path_buf()];
    }
    items
        .iter()
        .filter(|item| selected_paths.contains(&item.path))
        .map(|item| item.path.clone())
        .collect()
}

unsafe fn start_file_drag(window: HWND, paths: Vec<PathBuf>) -> Result<(), String> {
    if paths.len() > MAX_DROP_FILES as usize {
        return Err(format!("一次最多拖出 {MAX_DROP_FILES} 个项目"));
    }
    let paths: Vec<PathBuf> = paths.into_iter().filter(|path| path.exists()).collect();
    if paths.is_empty() {
        return Err("所选文件或文件夹已经不存在".into());
    }
    let data_object: IDataObject = FileDataObject { paths }.into();
    let drop_source: IDropSource = FileDropSource.into();
    let mut effect = DROPEFFECT_NONE;
    let result = DoDragDrop(
        &data_object,
        &drop_source,
        DROPEFFECT_COPY | DROPEFFECT_MOVE,
        &mut effect,
    );
    if result.is_err() {
        return Err(Error::from(result).to_string());
    }
    if result == DRAGDROP_S_DROP {
        refresh_fence(window, true);
    }
    Ok(())
}

unsafe fn begin_marquee_selection(window: HWND, point: POINT, buttons: WPARAM) -> bool {
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return false;
    }
    let content_top = match state_mut(window) {
        Some(WindowState::Fence(state)) => content_top(state.preferences.show_fence_titles),
        _ => return false,
    };
    let content = selection_content_rect(&client, content_top);
    if !point_in_rect(point, &content) {
        return false;
    }
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state))
            if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) =>
        {
            let modifiers = selection_modifiers(buttons);
            let base_selection = if modifiers.control || modifiers.shift {
                state.selected_paths.clone()
            } else {
                state.selection_anchor = None;
                state.focused_path = None;
                HashSet::new()
            };
            let changed = state.selected_paths != base_selection;
            state.selected_paths = base_selection.clone();
            state.marquee = Some(MarqueeSelection {
                start: point,
                current: point,
                base_selection,
            });
            changed
        }
        _ => return false,
    };
    if changed {
        let _ = InvalidateRect(Some(window), None, false);
    }
    let _ = SetFocus(Some(window));
    let _ = SetCapture(window);
    true
}

unsafe fn update_marquee_selection(window: HWND, point: POINT, buttons: WPARAM) -> bool {
    if buttons.0 & MK_LBUTTON.0 as usize == 0 {
        let finished = finish_marquee_selection(window);
        if finished && GetCapture() == window {
            let _ = ReleaseCapture();
        }
        return finished;
    }
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return false;
    }
    match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let Some(marquee) = state.marquee.as_mut() else {
                return false;
            };
            marquee.current = point;
            let selection_rect = selection_rectangle(marquee.start, point);
            let mut selected_paths = marquee.base_selection.clone();
            for cell in item_cells(
                &client,
                content_top(state.preferences.show_fence_titles),
                state.preferences.icon_size,
                state.items.len(),
                state.scroll_row,
            ) {
                if rects_intersect(&selection_rect, &cell.bounds) {
                    selected_paths.insert(state.items[cell.index].path.clone());
                }
            }
            state.selected_paths = selected_paths;
        }
        _ => return false,
    }
    let _ = InvalidateRect(Some(window), None, false);
    true
}

unsafe fn finish_marquee_selection(window: HWND) -> bool {
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            if state.marquee.take().is_none() {
                return false;
            }
            if state.selection_anchor.is_none() {
                state.selection_anchor = state
                    .items
                    .iter()
                    .rev()
                    .find(|item| state.selected_paths.contains(&item.path))
                    .map(|item| item.path.clone());
            }
            state.focused_path = state
                .items
                .iter()
                .rev()
                .find(|item| state.selected_paths.contains(&item.path))
                .map(|item| item.path.clone())
                .or_else(|| state.selection_anchor.clone());
            true
        }
        _ => return false,
    };
    if changed {
        let _ = InvalidateRect(Some(window), None, false);
    }
    true
}

unsafe fn cancel_marquee_selection(window: HWND) -> bool {
    let canceled = match state_mut(window) {
        Some(WindowState::Fence(state)) => state.marquee.take().is_some(),
        _ => false,
    };
    if canceled {
        let _ = InvalidateRect(Some(window), None, false);
    }
    canceled
}

unsafe fn interaction_mode_at(window: HWND, cursor: POINT) -> Option<InteractionMode> {
    let state = match state_mut(window) {
        Some(WindowState::Fence(state)) if !state.snapshot.locked => state,
        _ => return None,
    };
    let mut rect = RECT::default();
    if GetWindowRect(window, &mut rect).is_err()
        || cursor.x < rect.left
        || cursor.x >= rect.right
        || cursor.y < rect.top
        || cursor.y >= rect.bottom
    {
        return None;
    }
    if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) {
        let edges = ResizeEdges {
            left: cursor.x < rect.left + RESIZE_BORDER,
            top: cursor.y < rect.top + RESIZE_BORDER,
            right: cursor.x >= rect.right - RESIZE_BORDER,
            bottom: cursor.y >= rect.bottom - RESIZE_BORDER,
        };
        if edges.left || edges.top || edges.right || edges.bottom {
            return Some(InteractionMode::Resize(edges));
        }
    }
    (cursor.y < rect.top + title_bar_height(state.preferences.show_fence_titles))
        .then_some(InteractionMode::Move)
}

fn interaction_rect(
    mode: InteractionMode,
    start_cursor: POINT,
    start: RECT,
    cursor: POINT,
    min_height: i32,
    max_height: i32,
) -> RECT {
    let delta_x = cursor.x.saturating_sub(start_cursor.x);
    let delta_y = cursor.y.saturating_sub(start_cursor.y);
    match mode {
        InteractionMode::Move => {
            let width = start.right - start.left;
            let height = start.bottom - start.top;
            let left = start.left.saturating_add(delta_x).clamp(-32_768, 32_768);
            let top = start.top.saturating_add(delta_y).clamp(-32_768, 32_768);
            RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            }
        }
        InteractionMode::Resize(edges) => {
            let mut rect = start;
            if edges.left {
                rect.left = start
                    .left
                    .saturating_add(delta_x)
                    .clamp(start.right - MAX_FENCE_WIDTH, start.right - MIN_FENCE_WIDTH)
                    .clamp(-32_768, 32_768);
            }
            if edges.right {
                rect.right = start
                    .right
                    .saturating_add(delta_x)
                    .clamp(start.left + MIN_FENCE_WIDTH, start.left + MAX_FENCE_WIDTH);
            }
            if edges.top {
                rect.top = start
                    .top
                    .saturating_add(delta_y)
                    .clamp(start.bottom - max_height, start.bottom - min_height)
                    .clamp(-32_768, 32_768);
            }
            if edges.bottom {
                rect.bottom = start
                    .bottom
                    .saturating_add(delta_y)
                    .clamp(start.top + min_height, start.top + max_height);
            }
            rect
        }
    }
}

fn nearest_snap(value: i32, candidates: impl IntoIterator<Item = i32>) -> i32 {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let distance = candidate.abs_diff(value);
            (distance <= SNAP_DISTANCE as u32).then_some((distance, candidate))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
        .unwrap_or(value)
}

fn snap_and_constrain_to_work_area(
    mode: InteractionMode,
    mut rect: RECT,
    work: &RECT,
    min_height: i32,
    max_height: i32,
) -> RECT {
    let work_width = (work.right - work.left).max(1);
    let work_height = (work.bottom - work.top).max(1);
    match mode {
        InteractionMode::Move => {
            let width = (rect.right - rect.left).min(work_width);
            let height = (rect.bottom - rect.top).min(work_height);
            rect.left = nearest_snap(rect.left, [work.left, work.right - width])
                .clamp(work.left, work.right - width);
            rect.top = nearest_snap(rect.top, [work.top, work.bottom - height])
                .clamp(work.top, work.bottom - height);
            rect.right = rect.left + width;
            rect.bottom = rect.top + height;
        }
        InteractionMode::Resize(edges) => {
            if edges.left {
                let maximum_left = rect.right.saturating_sub(MIN_FENCE_WIDTH).max(work.left);
                rect.left = nearest_snap(rect.left, [work.left]).clamp(work.left, maximum_left);
            }
            if edges.right {
                let minimum_right = rect.left.saturating_add(MIN_FENCE_WIDTH).min(work.right);
                rect.right =
                    nearest_snap(rect.right, [work.right]).clamp(minimum_right, work.right);
            }
            if edges.top {
                let maximum_top = rect.bottom.saturating_sub(min_height).max(work.top);
                rect.top = nearest_snap(rect.top, [work.top]).clamp(work.top, maximum_top);
            }
            if edges.bottom {
                let minimum_bottom = rect.top.saturating_add(min_height).min(work.bottom);
                rect.bottom =
                    nearest_snap(rect.bottom, [work.bottom]).clamp(minimum_bottom, work.bottom);
            }

            rect.left = rect.left.max(work.left);
            rect.top = rect.top.max(work.top);
            rect.right = rect.right.min(work.right);
            rect.bottom = rect.bottom.min(work.bottom);
            if rect.right - rect.left > MAX_FENCE_WIDTH {
                if edges.left {
                    rect.left = rect.right - MAX_FENCE_WIDTH;
                } else {
                    rect.right = rect.left + MAX_FENCE_WIDTH;
                }
            }
            let bounded_max_height = max_height.min(work_height);
            if rect.bottom - rect.top > bounded_max_height {
                if edges.top {
                    rect.top = rect.bottom - bounded_max_height;
                } else {
                    rect.bottom = rect.top + bounded_max_height;
                }
            }
        }
    }
    rect
}

fn snap_interaction_rect(
    mode: InteractionMode,
    mut rect: RECT,
    siblings: &[RECT],
    min_height: i32,
    max_height: i32,
) -> RECT {
    if siblings.is_empty() {
        return rect;
    }
    match mode {
        InteractionMode::Move => {
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            let x_candidates = siblings.iter().flat_map(|sibling| {
                let sibling_width = sibling.right - sibling.left;
                [
                    sibling.left,
                    sibling.right,
                    sibling.left - width,
                    sibling.right - width,
                    sibling.left + (sibling_width - width) / 2,
                ]
            });
            let y_candidates = siblings.iter().flat_map(|sibling| {
                let sibling_height = sibling.bottom - sibling.top;
                [
                    sibling.top,
                    sibling.bottom,
                    sibling.top - height,
                    sibling.bottom - height,
                    sibling.top + (sibling_height - height) / 2,
                ]
            });
            rect.left = nearest_snap(rect.left, x_candidates);
            rect.top = nearest_snap(rect.top, y_candidates);
            rect.right = rect.left + width;
            rect.bottom = rect.top + height;
        }
        InteractionMode::Resize(edges) => {
            if edges.left {
                let candidates = siblings.iter().flat_map(|sibling| {
                    [
                        sibling.left,
                        sibling.right,
                        rect.right - (sibling.right - sibling.left),
                    ]
                });
                let snapped = nearest_snap(rect.left, candidates);
                if (MIN_FENCE_WIDTH..=MAX_FENCE_WIDTH).contains(&(rect.right - snapped)) {
                    rect.left = snapped;
                }
            }
            if edges.right {
                let candidates = siblings.iter().flat_map(|sibling| {
                    [
                        sibling.left,
                        sibling.right,
                        rect.left + (sibling.right - sibling.left),
                    ]
                });
                let snapped = nearest_snap(rect.right, candidates);
                if (MIN_FENCE_WIDTH..=MAX_FENCE_WIDTH).contains(&(snapped - rect.left)) {
                    rect.right = snapped;
                }
            }
            if edges.top {
                let candidates = siblings.iter().flat_map(|sibling| {
                    [
                        sibling.top,
                        sibling.bottom,
                        rect.bottom - (sibling.bottom - sibling.top),
                    ]
                });
                let snapped = nearest_snap(rect.top, candidates);
                if (min_height..=max_height).contains(&(rect.bottom - snapped)) {
                    rect.top = snapped;
                }
            }
            if edges.bottom {
                let candidates = siblings.iter().flat_map(|sibling| {
                    [
                        sibling.top,
                        sibling.bottom,
                        rect.top + (sibling.bottom - sibling.top),
                    ]
                });
                let snapped = nearest_snap(rect.bottom, candidates);
                if (min_height..=max_height).contains(&(snapped - rect.top)) {
                    rect.bottom = snapped;
                }
            }
        }
    }
    rect
}

fn prevent_sibling_overlap(
    mode: InteractionMode,
    start: RECT,
    mut rect: RECT,
    siblings: &[RECT],
    min_height: i32,
    max_height: i32,
) -> RECT {
    if !siblings
        .iter()
        .any(|sibling| rects_intersect(&rect, sibling))
    {
        return rect;
    }

    let maximum_passes = siblings.len().saturating_mul(4).max(1);
    match mode {
        InteractionMode::Move => {
            let delta_x = rect.left.saturating_sub(start.left);
            let delta_y = rect.top.saturating_sub(start.top);
            for _ in 0..maximum_passes {
                let Some(sibling) = siblings
                    .iter()
                    .find(|sibling| rects_intersect(&rect, sibling))
                else {
                    return rect;
                };
                let mut corrections = Vec::with_capacity(2);
                if delta_x > 0 {
                    corrections.push((sibling.left.saturating_sub(rect.right), 0));
                } else if delta_x < 0 {
                    corrections.push((sibling.right.saturating_sub(rect.left), 0));
                }
                if delta_y > 0 {
                    corrections.push((0, sibling.top.saturating_sub(rect.bottom)));
                } else if delta_y < 0 {
                    corrections.push((0, sibling.bottom.saturating_sub(rect.top)));
                }
                let Some((shift_x, shift_y)) =
                    corrections.into_iter().min_by_key(|(shift_x, shift_y)| {
                        shift_x.unsigned_abs() + shift_y.unsigned_abs()
                    })
                else {
                    return start;
                };
                rect.left = rect.left.saturating_add(shift_x);
                rect.right = rect.right.saturating_add(shift_x);
                rect.top = rect.top.saturating_add(shift_y);
                rect.bottom = rect.bottom.saturating_add(shift_y);
            }
        }
        InteractionMode::Resize(edges) => {
            for _ in 0..maximum_passes {
                let Some(sibling) = siblings
                    .iter()
                    .find(|sibling| rects_intersect(&rect, sibling))
                else {
                    return rect;
                };
                let mut candidates = Vec::with_capacity(4);
                if edges.left {
                    let mut candidate = rect;
                    candidate.left = sibling.right;
                    candidates.push(candidate);
                }
                if edges.right {
                    let mut candidate = rect;
                    candidate.right = sibling.left;
                    candidates.push(candidate);
                }
                if edges.top {
                    let mut candidate = rect;
                    candidate.top = sibling.bottom;
                    candidates.push(candidate);
                }
                if edges.bottom {
                    let mut candidate = rect;
                    candidate.bottom = sibling.top;
                    candidates.push(candidate);
                }
                let Some(candidate) = candidates
                    .into_iter()
                    .filter(|candidate| {
                        let width = candidate.right - candidate.left;
                        let height = candidate.bottom - candidate.top;
                        (MIN_FENCE_WIDTH..=MAX_FENCE_WIDTH).contains(&width)
                            && (min_height..=max_height).contains(&height)
                            && !rects_intersect(candidate, sibling)
                    })
                    .min_by_key(|candidate| {
                        candidate.left.abs_diff(rect.left)
                            + candidate.top.abs_diff(rect.top)
                            + candidate.right.abs_diff(rect.right)
                            + candidate.bottom.abs_diff(rect.bottom)
                    })
                else {
                    return start;
                };
                rect = candidate;
            }
        }
    }

    if siblings
        .iter()
        .any(|sibling| rects_intersect(&rect, sibling))
    {
        start
    } else {
        rect
    }
}

unsafe fn sibling_window_rects(window: HWND) -> Vec<RECT> {
    let controller_window = match state_mut(window) {
        Some(WindowState::Fence(state)) => state.controller_window,
        _ => return Vec::new(),
    };
    let siblings = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => controller
            .windows
            .values()
            .copied()
            .filter(|candidate| *candidate != window)
            .collect::<Vec<_>>(),
        _ => return Vec::new(),
    };
    siblings
        .into_iter()
        .filter_map(|sibling| {
            let mut rect = RECT::default();
            GetWindowRect(sibling, &mut rect).ok().map(|()| rect)
        })
        .collect()
}

fn rects_equal(left: &RECT, right: &RECT) -> bool {
    left.left == right.left
        && left.top == right.top
        && left.right == right.right
        && left.bottom == right.bottom
}

fn wheel_delta(wparam: WPARAM) -> i32 {
    i32::from(((wparam.0 >> 16) as u16) as i16)
}

unsafe fn scroll_fence(window: HWND, delta: i32) -> bool {
    if delta == 0 {
        return false;
    }
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return false;
    }
    let changed = match state_mut(window) {
        Some(WindowState::Fence(state))
            if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) =>
        {
            let Some(metrics) = grid_metrics(
                &client,
                content_top(state.preferences.show_fence_titles),
                state.preferences.icon_size,
                state.items.len(),
            ) else {
                return false;
            };
            if metrics.max_scroll_row == 0 {
                state.scroll_row = 0;
                state.wheel_delta_remainder = 0;
                return false;
            }
            state.wheel_delta_remainder = state.wheel_delta_remainder.saturating_add(delta);
            let notches = state.wheel_delta_remainder / WHEEL_DELTA;
            state.wheel_delta_remainder %= WHEEL_DELTA;
            if notches == 0 {
                return true;
            }
            let previous = state.scroll_row;
            state.scroll_row = wheel_scroll_row(state.scroll_row, notches, metrics);
            state.scroll_row != previous
        }
        _ => return false,
    };
    if changed {
        let _ = InvalidateRect(Some(window), None, false);
    }
    true
}

unsafe fn clamp_fence_scroll(window: HWND) {
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return;
    }
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        if effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) {
            return;
        }
        let max_scroll_row = grid_metrics(
            &client,
            content_top(state.preferences.show_fence_titles),
            state.preferences.icon_size,
            state.items.len(),
        )
        .map(|metrics| metrics.max_scroll_row)
        .unwrap_or_default();
        state.scroll_row = state.scroll_row.min(max_scroll_row);
        if max_scroll_row == 0 {
            state.wheel_delta_remainder = 0;
        }
    }
}

unsafe fn refresh_fence(window: HWND, force: bool) {
    let folder = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            if !force
                && (state.interaction.is_some()
                    || state.item_drag.is_some()
                    || state.marquee.is_some()
                    || state.active_shell_menu.is_some())
            {
                return;
            }
            let interval = if state.items.len() >= 1_000 {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(5)
            };
            if !force && state.last_folder_refresh.elapsed() < interval {
                return;
            }
            state.last_folder_refresh = Instant::now();
            state.snapshot.directory.clone()
        }
        _ => return,
    };
    let show_hidden_files = match state_mut(window) {
        Some(WindowState::Fence(state)) => state.preferences.show_hidden_files,
        _ => return,
    };
    let items = list_folder(&folder, show_hidden_files);
    let mut changed = false;
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        if force && !state.visuals.is_empty() {
            state.visuals.clear();
            changed = true;
        }
        if items != state.items {
            let existing_paths: HashSet<&Path> =
                items.iter().map(|item| item.path.as_path()).collect();
            state
                .selected_paths
                .retain(|path| existing_paths.contains(path.as_path()));
            if state
                .selection_anchor
                .as_ref()
                .is_some_and(|path| !existing_paths.contains(path.as_path()))
            {
                state.selection_anchor = None;
            }
            if state
                .focused_path
                .as_ref()
                .is_some_and(|path| !existing_paths.contains(path.as_path()))
            {
                state.focused_path = None;
            }
            state.item_drag = None;
            state.marquee = None;
            state.items = items;
            state.visuals.clear();
            changed = true;
        }
    }
    if changed {
        clamp_fence_scroll(window);
        let _ = InvalidateRect(Some(window), None, false);
    }
}

unsafe fn refresh_fence_from_watch(window: HWND) {
    let should_refresh = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            if state.last_folder_refresh.elapsed() < Duration::from_millis(200) {
                false
            } else {
                state.last_folder_refresh = Instant::now() - Duration::from_secs(30);
                true
            }
        }
        _ => false,
    };
    if should_refresh {
        refresh_fence(window, false);
    }
}

unsafe fn shutdown(controller_window: HWND) {
    log::info!(target: "shutdown", "native windows and keyboard hook cleanup started");
    emit_event(&HostEvent::Stopped);
    let (windows, keyboard_hook) = match state_mut(controller_window) {
        Some(WindowState::Controller(controller)) => {
            controller.hotkey_chord = None;
            (
                std::mem::take(&mut controller.windows),
                controller.keyboard_hook.take(),
            )
        }
        _ => (HashMap::new(), None),
    };
    GHOST_KEYBOARD_HOOK_STATE.with(|state| {
        *state.borrow_mut() = GhostKeyboardHookState::default();
    });
    if let Some(keyboard_hook) = keyboard_hook {
        if let Err(error) = UnhookWindowsHookEx(keyboard_hook) {
            log::warn!(target: "hotkey", "keyboard hook removal failed: {error}");
        } else {
            log::info!(target: "hotkey", "keyboard hook removed");
        }
    }
    for window in windows.into_values() {
        let _ = DestroyWindow(window);
    }
    let _ = DestroyWindow(controller_window);
    log::info!(target: "shutdown", "native windows cleanup completed");
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut WindowState> {
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut WindowState;
    pointer.as_mut()
}

unsafe fn paint_fence(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let _ = BeginPaint(window, &mut paint);
    let _ = EndPaint(window, &paint);
    let _ = render_fence_layered(window);
}

unsafe fn render_fence_layered(window: HWND) -> Result<(), String> {
    let mut client = RECT::default();
    GetClientRect(window, &mut client).map_err(display_windows_error)?;
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    if width <= 0 || height <= 0 {
        return Ok(());
    }
    let byte_length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Desktop Host 盒子绘制缓冲区过大".to_string())?;

    let memory_dc = CreateCompatibleDC(None);
    if memory_dc.0.is_null() {
        return Err(display_windows_error(Error::from_thread()));
    }
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: byte_length.min(u32::MAX as usize) as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut::<c_void>();
    let bitmap = match CreateDIBSection(
        Some(memory_dc),
        &bitmap_info,
        DIB_RGB_COLORS,
        &mut bits,
        None,
        0,
    ) {
        Ok(bitmap) => bitmap,
        Err(error) => {
            let _ = DeleteDC(memory_dc);
            return Err(display_windows_error(error));
        }
    };
    let previous_bitmap = SelectObject(memory_dc, HGDIOBJ(bitmap.0));

    let (
        content_color,
        frosted_content,
        title_alpha,
        content_alpha,
        surface_alpha,
        chrome_height,
        item_icon_size,
        item_overlays,
    ) = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let header_color = color_for_name(&state.snapshot.color);
            let content_color = content_color_for_name(&state.snapshot.content_color);
            let frosted_content = state.snapshot.content_color == "frosted";
            let title_alpha = panel_background_alpha(state.preferences.title_opacity);
            let content_alpha = panel_background_alpha(state.preferences.content_opacity);
            let surface_alpha = ghost_surface_alpha(
                state.preferences.ghost_mode
                    && state.preferences.ghost_mode_trigger == GhostModeTrigger::Automatic,
                state.mouse_inside,
                state.preferences.ghost_opacity,
            );
            let chrome_height = title_bar_height(state.preferences.show_fence_titles);
            let item_icon_size = state.preferences.icon_size.clamp(36, 64);
            let item_overlays =
                draw_fence_surface(memory_dc, &client, state, header_color, content_color);
            (
                content_color,
                frosted_content,
                title_alpha,
                content_alpha,
                surface_alpha,
                chrome_height,
                item_icon_size,
                item_overlays,
            )
        }
        _ => {
            let _ = SelectObject(memory_dc, previous_bitmap);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory_dc);
            return Err("Desktop Host 盒子窗口状态无效".into());
        }
    };

    if bits.is_null() {
        let _ = SelectObject(memory_dc, previous_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory_dc);
        return Err("Desktop Host 没有获得盒子绘制缓冲区".into());
    }
    let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), byte_length);
    apply_surface_alpha(
        pixels,
        content_color,
        title_alpha,
        content_alpha,
        surface_alpha,
        width as usize,
        chrome_height,
        frosted_content,
    );

    let update_result = composite_desktop_item_layer(
        memory_dc,
        pixels,
        width,
        height,
        &item_overlays.rectangles,
        &item_overlays.visuals,
        &item_overlays.texts,
        item_icon_size,
        surface_alpha,
    )
    .and_then(|()| {
        let mut window_rect = RECT::default();
        GetWindowRect(window, &mut window_rect).map_err(display_windows_error)?;
        let destination = POINT {
            x: window_rect.left,
            y: window_rect.top,
        };
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let source = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            window,
            None,
            Some(&destination),
            Some(&size),
            Some(memory_dc),
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
        .map_err(display_windows_error)
    });

    let _ = SelectObject(memory_dc, previous_bitmap);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(memory_dc);
    update_result
}

unsafe fn draw_fence_surface(
    dc: windows::Win32::Graphics::Gdi::HDC,
    client: &RECT,
    state: &mut FenceState,
    header_color: COLORREF,
    content_color: COLORREF,
) -> DesktopItemOverlays {
    let mut overlays = DesktopItemOverlays::default();
    if let Some(border) = fence_border_overlay(
        client,
        header_color,
        state.preferences.show_fence_border,
        state.preferences.fence_border_opacity,
    ) {
        overlays.rectangles.push(border);
    }
    let panel_brush = CreateSolidBrush(content_color);
    FillRect(dc, client, panel_brush);
    let _ = DeleteObject(HGDIOBJ(panel_brush.0));

    let previous_font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
    SetBkMode(dc, TRANSPARENT);
    let header = RECT {
        left: client.left,
        top: client.top,
        right: client.right,
        bottom: title_bar_height(state.preferences.show_fence_titles),
    };
    let header_brush = CreateSolidBrush(header_color);
    FillRect(dc, &header, header_brush);
    let _ = DeleteObject(HGDIOBJ(header_brush.0));
    if state.preferences.show_fence_titles {
        SetTextColor(dc, rgb(255, 255, 255));
        let mut title_rect = RECT {
            left: 18,
            top: 0,
            right: client.right - 92,
            bottom: HEADER_HEIGHT,
        };
        draw_text(
            dc,
            &state.snapshot.title,
            &mut title_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let mut count_rect = RECT {
            left: client.right - 88,
            top: 0,
            right: client.right - 14,
            bottom: HEADER_HEIGHT,
        };
        draw_text(
            dc,
            &if state.selected_paths.is_empty() {
                format!("{} 项", state.items.len())
            } else {
                format!("{}/{} 已选", state.selected_paths.len(), state.items.len())
            },
            &mut count_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }

    if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) {
        SetTextColor(dc, rgb(61, 57, 52));
        let icon_size = state.preferences.icon_size.clamp(36, 64);
        let content_top = content_top(state.preferences.show_fence_titles);
        let cells = item_cells(
            client,
            content_top,
            icon_size,
            state.items.len(),
            state.scroll_row,
        );
        let mut visible_visuals = HashSet::with_capacity(cells.len());
        if let Some(marquee) = state.marquee.as_ref()
            && let Some(overlay) = marquee_overlay(client, marquee, content_top)
        {
            overlays.rectangles.push(overlay);
        }
        for cell in cells {
            let item = state.items[cell.index].clone();
            if state.selected_paths.contains(&item.path) {
                overlays.rectangles.push(selection_overlay(&cell.bounds));
            }
            let key = VisualKey {
                path: item.path.clone(),
                size: icon_size,
            };
            visible_visuals.insert(key.clone());
            let visual = state
                .visuals
                .entry(key)
                .or_insert_with(|| extract_shell_visual(&item.path, icon_size));
            if let Some(visual) = visual.as_ref() {
                // Draw shell visuals after the panel Alpha has been applied. Drawing
                // them onto the opaque panel first permanently mixes the panel color
                // into anti-aliased icon edges and leaves a pale fringe when the panel
                // later becomes transparent.
                overlays.visuals.push(DesktopVisualOverlay {
                    handle: visual.handle,
                    width: visual.width,
                    height: visual.height,
                    kind: visual.kind,
                    uses_alpha: visual.uses_alpha,
                    bounds: cell.icon,
                });
            } else {
                overlays.texts.push(DesktopTextOverlay {
                    value: if item.is_dir { "[目录]" } else { "[文件]" }.into(),
                    rect: cell.icon,
                });
            }
            overlays.texts.push(DesktopTextOverlay {
                value: item.name,
                rect: cell.label,
            });
            if state.focused_path.as_ref() == Some(&item.path) {
                draw_item_focus(dc, &cell.bounds);
            }
        }
        if state.visuals.len() > MAX_VISUAL_CACHE {
            state.visuals.retain(|key, _| visible_visuals.contains(key));
        }
        if state.items.is_empty() {
            SetTextColor(dc, rgb(61, 57, 52));
            let mut empty = RECT {
                left: 16,
                top: content_top + 6,
                right: client.right - 16,
                bottom: client.bottom - 12,
            };
            draw_text(
                dc,
                "文件夹是空的",
                &mut empty,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }
    let _ = SelectObject(dc, previous_font);
    overlays
}

fn fence_border_overlay(
    bounds: &RECT,
    color: COLORREF,
    visible: bool,
    opacity: f64,
) -> Option<DesktopRectangleOverlay> {
    visible.then(|| DesktopRectangleOverlay {
        bounds: *bounds,
        fill_color: color,
        fill_alpha: 0,
        border_color: color,
        border_alpha: opacity_alpha(opacity),
    })
}

// These independent alpha and layout values are intentionally explicit at the
// pixel-processing boundary so call sites cannot accidentally swap units.
#[allow(clippy::too_many_arguments)]
fn apply_surface_alpha(
    pixels: &mut [u8],
    content_color: COLORREF,
    title_alpha: u8,
    content_alpha: u8,
    surface_alpha: u8,
    pixel_width: usize,
    chrome_height: i32,
    frosted_content: bool,
) {
    let content = dib_color(content_color);
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let is_chrome = pixel_width > 0 && index / pixel_width < chrome_height.max(0) as usize;
        if frosted_content && !is_chrome && pixel[..3] == content {
            let variation = match index.wrapping_mul(0x9e37_79b1) & 0x0f {
                0 | 7 => 6,
                3 | 11 => -4,
                _ => 0,
            };
            for channel in &mut pixel[..3] {
                *channel = i16::from(*channel).saturating_add(variation).clamp(0, 255) as u8;
            }
        }
        let base_alpha = if is_chrome {
            title_alpha
        } else {
            content_alpha
        };
        let scaled_alpha = ((u16::from(base_alpha) * u16::from(surface_alpha) + 127) / 255) as u8;
        let alpha = if surface_alpha > 0 {
            scaled_alpha.max(1)
        } else {
            scaled_alpha
        };
        if alpha != 255 {
            pixel[0] = ((u16::from(pixel[0]) * u16::from(alpha) + 127) / 255) as u8;
            pixel[1] = ((u16::from(pixel[1]) * u16::from(alpha) + 127) / 255) as u8;
            pixel[2] = ((u16::from(pixel[2]) * u16::from(alpha) + 127) / 255) as u8;
        }
        pixel[3] = alpha;
    }
}

// This is the single boundary between GDI resources and the software BGRA
// compositor; keeping the buffers and dimensions explicit makes ownership clear.
#[allow(clippy::too_many_arguments)]
unsafe fn composite_desktop_item_layer(
    reference_dc: windows::Win32::Graphics::Gdi::HDC,
    destination: &mut [u8],
    width: i32,
    height: i32,
    rectangle_overlays: &[DesktopRectangleOverlay],
    visual_overlays: &[DesktopVisualOverlay],
    text_overlays: &[DesktopTextOverlay],
    icon_size: u32,
    surface_alpha: u8,
) -> Result<(), String> {
    let byte_length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Desktop Host 项目透明缓冲区过大".to_string())?;
    if destination.len() != byte_length {
        return Err("Desktop Host 项目透明缓冲区尺寸无效".into());
    }
    for overlay in rectangle_overlays {
        composite_rectangle_overlay(destination, width, height, overlay, surface_alpha);
    }
    if visual_overlays.is_empty() && text_overlays.is_empty() {
        return Ok(());
    }

    let layer_dc = CreateCompatibleDC(Some(reference_dc));
    if layer_dc.0.is_null() {
        return Err(display_windows_error(Error::from_thread()));
    }
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: byte_length.min(u32::MAX as usize) as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut layer_bits = std::ptr::null_mut::<c_void>();
    let layer_bitmap = match CreateDIBSection(
        Some(layer_dc),
        &bitmap_info,
        DIB_RGB_COLORS,
        &mut layer_bits,
        None,
        0,
    ) {
        Ok(bitmap) => bitmap,
        Err(error) => {
            let _ = DeleteDC(layer_dc);
            return Err(display_windows_error(error));
        }
    };
    let previous_bitmap = SelectObject(layer_dc, HGDIOBJ(layer_bitmap.0));
    if layer_bits.is_null() {
        let _ = SelectObject(layer_dc, previous_bitmap);
        let _ = DeleteObject(HGDIOBJ(layer_bitmap.0));
        let _ = DeleteDC(layer_dc);
        return Err("Desktop Host 没有获得项目透明缓冲区".into());
    }

    let desktop_font = desktop_icon_font(icon_size);
    let previous_font = desktop_font.map(|font| SelectObject(layer_dc, HGDIOBJ(font.0)));
    SetBkMode(layer_dc, TRANSPARENT);
    let layer_bounds = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };

    let black_brush = CreateSolidBrush(rgb(0, 0, 0));
    FillRect(layer_dc, &layer_bounds, black_brush);
    let _ = DeleteObject(HGDIOBJ(black_brush.0));
    for overlay in visual_overlays {
        let _ = draw_visual_overlay(layer_dc, overlay);
    }
    for overlay in text_overlays {
        draw_desktop_item_label(layer_dc, &overlay.value, &overlay.rect);
    }
    let _ = GdiFlush();
    let black_pixels = std::slice::from_raw_parts(layer_bits.cast::<u8>(), byte_length).to_vec();

    let white_brush = CreateSolidBrush(rgb(255, 255, 255));
    FillRect(layer_dc, &layer_bounds, white_brush);
    let _ = DeleteObject(HGDIOBJ(white_brush.0));
    for overlay in visual_overlays {
        let _ = draw_visual_overlay(layer_dc, overlay);
    }
    for overlay in text_overlays {
        draw_desktop_item_label(layer_dc, &overlay.value, &overlay.rect);
    }
    let _ = GdiFlush();
    let white_pixels = std::slice::from_raw_parts(layer_bits.cast::<u8>(), byte_length);

    for ((destination, black), white) in destination
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(black_pixels.as_chunks::<4>().0)
        .zip(white_pixels.as_chunks::<4>().0)
    {
        composite_reconstructed_pixel(destination, black, white, surface_alpha);
    }

    if let Some(previous_font) = previous_font {
        let _ = SelectObject(layer_dc, previous_font);
    }
    if let Some(desktop_font) = desktop_font {
        let _ = DeleteObject(HGDIOBJ(desktop_font.0));
    }
    let _ = SelectObject(layer_dc, previous_bitmap);
    let _ = DeleteObject(HGDIOBJ(layer_bitmap.0));
    let _ = DeleteDC(layer_dc);
    Ok(())
}

fn composite_reconstructed_pixel(
    destination: &mut [u8],
    black: &[u8],
    white: &[u8],
    surface_alpha: u8,
) {
    let blue_alpha = 255u8.saturating_sub(white[0].saturating_sub(black[0]));
    let green_alpha = 255u8.saturating_sub(white[1].saturating_sub(black[1]));
    let red_alpha = 255u8.saturating_sub(white[2].saturating_sub(black[2]));
    let content_alpha = blue_alpha.max(green_alpha).max(red_alpha);
    if content_alpha == 0 {
        return;
    }

    let inverse_content_alpha = 255u16 - u16::from(content_alpha);
    for channel in 0..3 {
        let scaled_content = (u16::from(black[channel]) * u16::from(surface_alpha) + 127) / 255;
        let retained_destination =
            (u16::from(destination[channel]) * inverse_content_alpha + 127) / 255;
        destination[channel] = scaled_content.saturating_add(retained_destination).min(255) as u8;
    }
    let scaled_content_alpha = (u16::from(content_alpha) * u16::from(surface_alpha) + 127) / 255;
    let retained_destination_alpha =
        (u16::from(destination[3]) * inverse_content_alpha + 127) / 255;
    destination[3] = scaled_content_alpha
        .saturating_add(retained_destination_alpha)
        .min(255) as u8;
}

fn composite_rectangle_overlay(
    destination: &mut [u8],
    width: i32,
    height: i32,
    overlay: &DesktopRectangleOverlay,
    surface_alpha: u8,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    let left = overlay.bounds.left.clamp(0, width);
    let top = overlay.bounds.top.clamp(0, height);
    let right = overlay.bounds.right.clamp(0, width);
    let bottom = overlay.bounds.bottom.clamp(0, height);
    if left >= right || top >= bottom {
        return;
    }
    let fill_alpha = ((u16::from(overlay.fill_alpha) * u16::from(surface_alpha) + 127) / 255) as u8;
    let border_alpha =
        ((u16::from(overlay.border_alpha) * u16::from(surface_alpha) + 127) / 255) as u8;
    let width = width as usize;
    for y in top..bottom {
        for x in left..right {
            let border = x == left || x == right - 1 || y == top || y == bottom - 1;
            let (color, alpha) = if border && border_alpha > 0 {
                (overlay.border_color, border_alpha)
            } else {
                (overlay.fill_color, fill_alpha)
            };
            if alpha == 0 {
                continue;
            }
            let index = (y as usize * width + x as usize) * 4;
            composite_color_pixel(&mut destination[index..index + 4], color, alpha);
        }
    }
}

fn composite_color_pixel(destination: &mut [u8], color: COLORREF, alpha: u8) {
    let color = dib_color(color);
    let inverse_alpha = 255u16 - u16::from(alpha);
    for channel in 0..3 {
        let source = (u16::from(color[channel]) * u16::from(alpha) + 127) / 255;
        let retained = (u16::from(destination[channel]) * inverse_alpha + 127) / 255;
        destination[channel] = source.saturating_add(retained).min(255) as u8;
    }
    let retained_alpha = (u16::from(destination[3]) * inverse_alpha + 127) / 255;
    destination[3] = u16::from(alpha).saturating_add(retained_alpha).min(255) as u8;
}

fn dib_color(color: COLORREF) -> [u8; 3] {
    let value = color.0;
    [
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ]
}

fn selection_overlay(bounds: &RECT) -> DesktopRectangleOverlay {
    DesktopRectangleOverlay {
        bounds: *bounds,
        fill_color: rgb(91, 144, 184),
        fill_alpha: 42,
        border_color: rgb(91, 144, 184),
        border_alpha: 132,
    }
}

unsafe fn draw_item_focus(dc: windows::Win32::Graphics::Gdi::HDC, bounds: &RECT) {
    let focus = RECT {
        left: bounds.left.saturating_add(3),
        top: bounds.top.saturating_add(3),
        right: bounds.right.saturating_sub(3),
        bottom: bounds.bottom.saturating_sub(3),
    };
    if focus.left < focus.right && focus.top < focus.bottom {
        let _ = DrawFocusRect(dc, &focus);
    }
}

fn marquee_overlay(
    client: &RECT,
    marquee: &MarqueeSelection,
    content_top: i32,
) -> Option<DesktopRectangleOverlay> {
    let selection = selection_rectangle(marquee.start, marquee.current);
    let content = selection_content_rect(client, content_top);
    let clipped = RECT {
        left: selection.left.max(content.left),
        top: selection.top.max(content.top),
        right: selection.right.min(content.right),
        bottom: selection.bottom.min(content.bottom),
    };
    if clipped.left >= clipped.right || clipped.top >= clipped.bottom {
        return None;
    }
    Some(DesktopRectangleOverlay {
        bounds: clipped,
        fill_color: rgb(91, 144, 184),
        fill_alpha: 30,
        border_color: rgb(91, 144, 184),
        border_alpha: 148,
    })
}

unsafe fn paint_rename_dialog(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let dc = BeginPaint(window, &mut paint);
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_ok() {
        let brush = CreateSolidBrush(rgb(245, 239, 229));
        FillRect(dc, &client, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
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

unsafe fn desktop_icon_font(icon_size: u32) -> Option<HFONT> {
    let mut font = LOGFONTW::default();
    SystemParametersInfoW(
        SPI_GETICONTITLELOGFONT,
        std::mem::size_of::<LOGFONTW>() as u32,
        Some((&mut font as *mut LOGFONTW).cast::<c_void>()),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
    .ok()?;
    // ClearType 使用每个颜色通道不同的覆盖率，不适合透明分层重建；灰度
    // 抗锯齿仍保持圆滑，同时能从黑白双缓冲精确反算统一 Alpha。
    font.lfQuality = ANTIALIASED_QUALITY;
    font.lfHeight = scale_desktop_font_height(font.lfHeight, icon_size);
    let handle = CreateFontIndirectW(&font);
    (!handle.0.is_null()).then_some(handle)
}

unsafe fn draw_desktop_item_label(
    dc: windows::Win32::Graphics::Gdi::HDC,
    value: &str,
    rect: &RECT,
) {
    let mut shadow = RECT {
        left: rect.left + 1,
        top: rect.top + 1,
        right: rect.right + 1,
        bottom: rect.bottom + 1,
    };
    SetTextColor(dc, rgb(24, 24, 24));
    draw_text(
        dc,
        value,
        &mut shadow,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    let mut foreground = *rect;
    SetTextColor(dc, rgb(255, 255, 255));
    draw_text(
        dc,
        value,
        &mut foreground,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

fn grid_metrics(
    client: &RECT,
    content_top: i32,
    icon_size: u32,
    item_count: usize,
) -> Option<GridMetrics> {
    let icon_size = icon_size.clamp(36, 64) as i32;
    let cell_width = icon_size + scale_item_metric(48, icon_size as u32);
    let row_height = icon_size + scale_item_metric(44, icon_size as u32);
    let content_width = client.right - client.left - CONTENT_LEFT - CONTENT_RIGHT;
    let content_height = client.bottom - client.top - content_top - CONTENT_BOTTOM;
    if content_width <= 0 || content_height <= 0 {
        return None;
    }
    let columns = (content_width / cell_width).max(1) as usize;
    let visible_rows = (content_height / row_height).max(1) as usize;
    let total_rows = item_count.div_ceil(columns);
    let page_step = scroll_page_step(visible_rows);
    let overflow_rows = total_rows.saturating_sub(visible_rows);
    Some(GridMetrics {
        columns,
        row_height,
        visible_rows,
        total_rows,
        // Each wheel page keeps the preceding page's last row at the top. Keep the
        // final page on that same page boundary instead of pulling earlier rows
        // forward just to fill otherwise empty space.
        max_scroll_row: overflow_rows.div_ceil(page_step).saturating_mul(page_step),
    })
}

fn scroll_page_step(visible_rows: usize) -> usize {
    visible_rows.saturating_sub(1).max(1)
}

fn wheel_scroll_row(current_scroll_row: usize, notches: i32, metrics: GridMetrics) -> usize {
    let rows_per_notch = scroll_page_step(metrics.visible_rows) as i64;
    let next = current_scroll_row as i64 - i64::from(notches) * rows_per_notch;
    next.clamp(0, metrics.max_scroll_row as i64) as usize
}

fn item_cells(
    client: &RECT,
    content_top: i32,
    icon_size: u32,
    item_count: usize,
    scroll_row: usize,
) -> Vec<ItemCell> {
    let Some(metrics) = grid_metrics(client, content_top, icon_size, item_count) else {
        return Vec::new();
    };
    let icon_size = icon_size.clamp(36, 64) as i32;
    let icon_top_padding = scale_item_metric(5, icon_size as u32);
    let label_gap = scale_item_metric(4, icon_size as u32);
    let label_horizontal_padding = scale_item_metric(6, icon_size as u32);
    let label_bottom_padding = scale_item_metric(4, icon_size as u32);
    let content_left = client.left + CONTENT_LEFT;
    let content_right = client.right - CONTENT_RIGHT;
    let content_width = content_right - content_left;
    let scroll_row = scroll_row.min(metrics.max_scroll_row);
    let first_index = scroll_row.saturating_mul(metrics.columns);
    let end_row = scroll_row
        .saturating_add(metrics.visible_rows)
        .min(metrics.total_rows);
    let end_index = end_row.saturating_mul(metrics.columns).min(item_count);
    let mut cells = Vec::with_capacity(end_index.saturating_sub(first_index));
    for index in first_index..end_index {
        let column = index % metrics.columns;
        let row = index / metrics.columns - scroll_row;
        // The nominal cell width decides how many columns fit. Once that count is
        // known, divide the complete content area between those columns so no
        // scrollbar-sized strip is left unused on the right.
        let left = content_left
            + (i64::from(content_width) * column as i64 / metrics.columns as i64) as i32;
        let right = content_left
            + (i64::from(content_width) * (column + 1) as i64 / metrics.columns as i64) as i32;
        let top = client.top + content_top + row as i32 * metrics.row_height;
        let actual_width = right - left;
        let icon_left = left + (actual_width - icon_size).max(0) / 2;
        cells.push(ItemCell {
            index,
            bounds: RECT {
                left,
                top,
                right,
                bottom: (top + metrics.row_height).min(client.bottom - CONTENT_BOTTOM),
            },
            icon: RECT {
                left: icon_left,
                top: top + icon_top_padding,
                right: icon_left + icon_size,
                bottom: top + icon_top_padding + icon_size,
            },
            label: RECT {
                left: left + label_horizontal_padding,
                top: top + icon_top_padding + icon_size + label_gap,
                right: right - label_horizontal_padding,
                bottom: (top + metrics.row_height - label_bottom_padding)
                    .min(client.bottom - CONTENT_BOTTOM),
            },
        });
    }
    cells
}

fn scale_item_metric(default_value: i32, icon_size: u32) -> i32 {
    let icon_size = icon_size.clamp(36, 64) as i64;
    ((i64::from(default_value) * icon_size + i64::from(DEFAULT_ITEM_ICON_SIZE / 2))
        / i64::from(DEFAULT_ITEM_ICON_SIZE))
    .max(1) as i32
}

fn scale_desktop_font_height(default_value: i32, icon_size: u32) -> i32 {
    if default_value == 0 {
        return 0;
    }
    let icon_size = icon_size.clamp(36, 64);
    let magnitude = default_value.saturating_abs() as f64;
    let scale = if icon_size <= DEFAULT_ITEM_ICON_SIZE as u32 {
        // 小图标继续使用紧凑网格，但文字只轻微缩小。按图标同比缩放会让
        // 36px 状态下系统桌面字体从约 18px 降到 14px，1px 阴影随即挤成一团。
        0.9 + f64::from(icon_size.saturating_sub(36)) * 0.01
    } else {
        f64::from(icon_size) / f64::from(DEFAULT_ITEM_ICON_SIZE)
    };
    let scaled = (magnitude * scale).round().max(1.0) as i32;
    if default_value < 0 { -scaled } else { scaled }
}

fn item_index_at(
    client: &RECT,
    content_top: i32,
    icon_size: u32,
    item_count: usize,
    scroll_row: usize,
    point: POINT,
) -> Option<usize> {
    item_cells(client, content_top, icon_size, item_count, scroll_row)
        .into_iter()
        .find(|cell| point_in_rect(point, &cell.bounds))
        .map(|cell| cell.index)
}

fn point_in_rect(point: POINT, rect: &RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn selection_content_rect(client: &RECT, content_top: i32) -> RECT {
    RECT {
        left: client.left + CONTENT_LEFT,
        top: client.top + content_top,
        right: (client.right - CONTENT_RIGHT).max(client.left + CONTENT_LEFT),
        bottom: (client.bottom - CONTENT_BOTTOM).max(client.top + content_top),
    }
}

fn selection_rectangle(start: POINT, current: POINT) -> RECT {
    RECT {
        left: start.x.min(current.x),
        top: start.y.min(current.y),
        right: start.x.max(current.x).saturating_add(1),
        bottom: start.y.max(current.y).saturating_add(1),
    }
}

fn rects_intersect(left: &RECT, right: &RECT) -> bool {
    left.left < right.right
        && left.right > right.left
        && left.top < right.bottom
        && left.bottom > right.top
}

fn client_point(lparam: LPARAM) -> POINT {
    let packed = lparam.0 as u32;
    POINT {
        x: (packed as u16 as i16) as i32,
        y: ((packed >> 16) as u16 as i16) as i32,
    }
}

unsafe fn context_menu_point(window: HWND, lparam: LPARAM) -> POINT {
    let packed = lparam.0 as u32;
    let mut point = POINT {
        x: (packed as u16 as i16) as i32,
        y: ((packed >> 16) as u16 as i16) as i32,
    };
    if point.x == -1 && point.y == -1 && GetCursorPos(&mut point).is_err() {
        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect).is_ok() {
            point.x = rect.left + (rect.right - rect.left) / 2;
            point.y = rect.top + HEADER_HEIGHT / 2;
        }
    }
    point
}

unsafe fn show_keyboard_context_menu(window: HWND) -> bool {
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return true;
    }
    let (mut point, scroll_changed) = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let item_index =
                (!effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles))
                    .then(|| {
                        state
                            .focused_path
                            .as_ref()
                            .and_then(|path| state.items.iter().position(|item| item.path == *path))
                            .or_else(|| {
                                state
                                    .items
                                    .iter()
                                    .position(|item| state.selected_paths.contains(&item.path))
                            })
                    })
                    .flatten();
            if let Some(index) = item_index {
                let previous_scroll = state.scroll_row;
                if let Some(metrics) = grid_metrics(
                    &client,
                    content_top(state.preferences.show_fence_titles),
                    state.preferences.icon_size,
                    state.items.len(),
                ) {
                    state.scroll_row = scroll_row_for_item(state.scroll_row, index, metrics);
                }
                let point = item_cells(
                    &client,
                    content_top(state.preferences.show_fence_titles),
                    state.preferences.icon_size,
                    state.items.len(),
                    state.scroll_row,
                )
                .into_iter()
                .find(|cell| cell.index == index)
                .map(|cell| POINT {
                    x: cell.bounds.left + (cell.bounds.right - cell.bounds.left) / 2,
                    y: cell.bounds.top + (cell.bounds.bottom - cell.bounds.top) / 2,
                })
                .unwrap_or(POINT {
                    x: client.left + (client.right - client.left) / 2,
                    y: client.top + HEADER_HEIGHT / 2,
                });
                (point, previous_scroll != state.scroll_row)
            } else {
                (
                    POINT {
                        x: client.left + (client.right - client.left) / 2,
                        y: client.top + HEADER_HEIGHT / 2,
                    },
                    false,
                )
            }
        }
        _ => return false,
    };
    if scroll_changed {
        let _ = InvalidateRect(Some(window), None, false);
    }
    if !ClientToScreen(window, &mut point).as_bool() {
        return true;
    }
    show_fence_context_menu(window, point);
    true
}

unsafe fn show_fence_context_menu(window: HWND, screen_point: POINT) {
    let mut client_point = screen_point;
    let _ = ScreenToClient(window, &mut client_point);
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return;
    }
    let (snapshot, item_menu, selection_changed) = match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let item_index =
                (!effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles))
                    .then(|| {
                        item_index_at(
                            &client,
                            content_top(state.preferences.show_fence_titles),
                            state.preferences.icon_size,
                            state.items.len(),
                            state.scroll_row,
                            client_point,
                        )
                    })
                    .flatten();
            let mut selection_changed = false;
            let item_menu = item_index.and_then(|index| {
                let item = state.items.get(index)?.clone();
                if !state.selected_paths.contains(&item.path) {
                    state.selected_paths.clear();
                    state.selected_paths.insert(item.path.clone());
                    state.selection_anchor = Some(item.path.clone());
                    selection_changed = true;
                }
                if state.focused_path.as_ref() != Some(&item.path) {
                    state.focused_path = Some(item.path.clone());
                    selection_changed = true;
                }
                let paths = selected_paths_in_item_order(
                    &state.items,
                    &state.selected_paths,
                    state.focused_path.as_deref(),
                );
                Some((item, paths))
            });
            (state.snapshot.clone(), item_menu, selection_changed)
        }
        _ => return,
    };
    if selection_changed {
        let _ = InvalidateRect(Some(window), None, false);
    }
    let _ = SetFocus(Some(window));
    let result = if let Some((item, paths)) = item_menu {
        show_item_menu(window, screen_point, &item, &paths)
    } else {
        show_box_menu(window, screen_point, &snapshot)
    };
    if let Err(error) = result {
        emit_event(&HostEvent::Notification {
            message: format!("无法显示右键菜单：{error}"),
        });
    }
}

unsafe fn show_item_menu(
    window: HWND,
    screen_point: POINT,
    item: &FolderItem,
    paths: &[PathBuf],
) -> Result<(), String> {
    let context_menu = match shell_context_menu(window, paths) {
        Ok(context_menu) => context_menu,
        Err(error) => {
            notify_shell_menu_fallback(&item.path, &error);
            return show_item_fallback_menu(window, screen_point, item);
        }
    };
    let menu = PopupMenu(CreatePopupMenu().map_err(display_windows_error)?);
    if let Err(error) = context_menu
        .QueryContextMenu(
            menu.0,
            0,
            SHELL_MENU_ID_FIRST,
            SHELL_MENU_ID_LAST,
            CMF_NORMAL | CMF_CANRENAME,
        )
        .ok()
    {
        notify_shell_menu_fallback(&item.path, &display_windows_error(error));
        return show_item_fallback_menu(window, screen_point, item);
    }

    let active = ActiveShellMenu {
        menu2: context_menu.cast::<IContextMenu2>().ok(),
        menu3: context_menu.cast::<IContextMenu3>().ok(),
    };
    match state_mut(window) {
        Some(WindowState::Fence(state)) => state.active_shell_menu = Some(active),
        _ => return Err("Desktop Host 盒子窗口状态无效".into()),
    }
    let command = track_popup_menu(window, menu.0, screen_point) as u32;
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.active_shell_menu.take();
    }
    if !(SHELL_MENU_ID_FIRST..=SHELL_MENU_ID_LAST).contains(&command) {
        return Ok(());
    }

    let verb = usize::try_from(command - SHELL_MENU_ID_FIRST)
        .map_err(|_| "Explorer 右键命令编号无效".to_string())?;
    let invocation = CMINVOKECOMMANDINFOEX {
        cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_PTINVOKE,
        hwnd: window,
        lpVerb: PCSTR(verb as *const u8),
        lpVerbW: PCWSTR(verb as *const u16),
        nShow: SW_SHOWNORMAL.0,
        ptInvoke: screen_point,
        ..Default::default()
    };
    let invoke_result = context_menu
        .InvokeCommand((&invocation as *const CMINVOKECOMMANDINFOEX).cast::<CMINVOKECOMMANDINFO>())
        .map_err(display_windows_error);
    refresh_fence(window, true);
    invoke_result
}

unsafe fn show_item_fallback_menu(
    window: HWND,
    screen_point: POINT,
    item: &FolderItem,
) -> Result<(), String> {
    let menu = PopupMenu(CreatePopupMenu().map_err(display_windows_error)?);
    append_menu_item(menu.0, MF_STRING, ITEM_MENU_OPEN, "打开")?;
    append_menu_item(
        menu.0,
        MF_STRING,
        ITEM_MENU_REVEAL,
        "在文件资源管理器中显示",
    )?;
    match track_popup_menu(window, menu.0, screen_point) {
        ITEM_MENU_OPEN => open_shell_path(window, &item.path),
        ITEM_MENU_REVEAL => reveal_shell_path(&item.path),
        _ => Ok(()),
    }
}

fn notify_shell_menu_fallback(path: &Path, error: &str) {
    emit_event(&HostEvent::Notification {
        message: format!(
            "无法加载 {} 的 Explorer 右键菜单，已使用简化菜单：{error}",
            path.display()
        ),
    });
}

unsafe fn shell_context_menu(window: HWND, paths: &[PathBuf]) -> Result<IContextMenu, String> {
    let Some(first_path) = paths.first() else {
        return Err("没有选中任何项目".into());
    };
    if paths.len() > MAX_DROP_FILES as usize {
        return Err(format!("一次最多操作 {MAX_DROP_FILES} 个项目"));
    }
    let first_parent = first_path
        .parent()
        .ok_or_else(|| "无法确定所选项目的父目录".to_string())?;
    let mut absolute_pidls = Vec::with_capacity(paths.len());
    let mut child_pidls = Vec::with_capacity(paths.len());
    let mut shell_parent = None;
    for path in paths {
        if !path.exists() {
            return Err(format!("文件或文件夹已经不存在：{}", path.display()));
        }
        if path.parent() != Some(first_parent) {
            return Err("Explorer 多选菜单要求所有项目位于同一文件夹".into());
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut absolute_pidl = std::ptr::null_mut();
        SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut absolute_pidl, 0, None)
            .map_err(display_windows_error)?;
        if absolute_pidl.is_null() {
            return Err(format!(
                "Windows Shell 没有返回项目标识符：{}",
                path.display()
            ));
        }
        let absolute_pidl = OwnedPidl(absolute_pidl);
        let mut child_pidl = std::ptr::null_mut();
        let parent: IShellFolder = SHBindToParent(absolute_pidl.0, Some(&mut child_pidl))
            .map_err(display_windows_error)?;
        if child_pidl.is_null() {
            return Err(format!(
                "Windows Shell 没有返回父目录中的项目标识符：{}",
                path.display()
            ));
        }
        if shell_parent.is_none() {
            shell_parent = Some(parent);
        }
        child_pidls.push(child_pidl.cast_const());
        absolute_pidls.push(absolute_pidl);
    }
    let shell_parent = shell_parent.ok_or_else(|| "Windows Shell 父目录无效".to_string())?;
    let context_menu = shell_parent
        .GetUIObjectOf::<IContextMenu>(window, &child_pidls, None)
        .map_err(display_windows_error)?;
    drop(absolute_pidls);
    Ok(context_menu)
}

unsafe fn invoke_shell_verb(window: HWND, paths: &[PathBuf], verb: &str) -> Result<(), String> {
    if verb.is_empty() || !verb.is_ascii() || verb.as_bytes().contains(&0) {
        return Err("Explorer Shell 动词无效".into());
    }
    let context_menu = shell_context_menu(window, paths)?;
    let menu = PopupMenu(CreatePopupMenu().map_err(display_windows_error)?);
    context_menu
        .QueryContextMenu(
            menu.0,
            0,
            SHELL_MENU_ID_FIRST,
            SHELL_MENU_ID_LAST,
            CMF_NORMAL,
        )
        .ok()
        .map_err(display_windows_error)?;
    let mut ansi_verb = verb.as_bytes().to_vec();
    ansi_verb.push(0);
    let wide_verb: Vec<u16> = verb.encode_utf16().chain(Some(0)).collect();
    let invocation = CMINVOKECOMMANDINFOEX {
        cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_UNICODE_FLAG,
        hwnd: window,
        lpVerb: PCSTR(ansi_verb.as_ptr()),
        lpVerbW: PCWSTR(wide_verb.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    let result = context_menu
        .InvokeCommand((&invocation as *const CMINVOKECOMMANDINFOEX).cast::<CMINVOKECOMMANDINFO>())
        .map_err(display_windows_error);
    refresh_fence(window, true);
    result
}

unsafe fn show_box_menu(
    window: HWND,
    screen_point: POINT,
    snapshot: &HostFenceSnapshot,
) -> Result<(), String> {
    let menu = PopupMenu(CreatePopupMenu().map_err(display_windows_error)?);
    let can_undo_geometry = matches!(
        state_mut(window),
        Some(WindowState::Fence(FenceState {
            undo_geometry: Some(_),
            ..
        }))
    );
    append_menu_item(menu.0, MF_STRING, FENCE_MENU_NEW, "新建收纳盒...")?;
    append_menu_item(menu.0, MF_STRING, FENCE_MENU_NEW_MAPPED, "新建映射盒子...")?;
    append_menu_item(
        menu.0,
        MF_STRING,
        FENCE_MENU_NEW_FOLDER,
        "在盒子内新建文件夹...",
    )?;
    append_menu_separator(menu.0)?;
    append_menu_item(menu.0, MF_STRING, FENCE_MENU_REFRESH, "刷新内容")?;
    append_menu_item(menu.0, MF_STRING, FENCE_MENU_OPEN, "打开文件夹")?;
    append_menu_item(menu.0, MF_STRING, FENCE_MENU_RENAME, "修改名称...")?;
    append_menu_separator(menu.0)?;
    append_menu_item(
        menu.0,
        if can_undo_geometry {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        },
        FENCE_MENU_UNDO_GEOMETRY,
        "撤销上次移动或缩放",
    )?;
    append_menu_item(menu.0, MF_STRING, FENCE_MENU_RESET_SIZE, "恢复默认大小")?;
    append_menu_item(
        menu.0,
        MF_STRING
            | if snapshot.collapsed {
                MF_CHECKED
            } else {
                MF_STRING
            },
        FENCE_MENU_COLLAPSE,
        if snapshot.collapsed {
            "展开盒子"
        } else {
            "收起盒子"
        },
    )?;
    append_menu_item(
        menu.0,
        MF_STRING
            | if snapshot.locked {
                MF_CHECKED
            } else {
                MF_STRING
            },
        FENCE_MENU_LOCK,
        "锁定位置",
    )?;

    let color_menu = CreatePopupMenu().map_err(display_windows_error)?;
    let colors = fence_colors();
    for (index, (color, label)) in colors.iter().enumerate() {
        let flags = MF_STRING
            | if snapshot.color == *color {
                MF_CHECKED
            } else {
                MF_STRING
            };
        if let Err(error) =
            append_menu_item(color_menu, flags, FENCE_MENU_COLOR_FIRST + index, label)
        {
            let _ = DestroyMenu(color_menu);
            return Err(error);
        }
    }
    let color_title: Vec<u16> = "盒子颜色".encode_utf16().chain(Some(0)).collect();
    if let Err(error) = AppendMenuW(
        menu.0,
        MF_POPUP,
        color_menu.0 as usize,
        PCWSTR(color_title.as_ptr()),
    ) {
        let _ = DestroyMenu(color_menu);
        return Err(display_windows_error(error));
    }
    append_menu_separator(menu.0)?;
    append_menu_item(
        menu.0,
        MF_STRING,
        FENCE_MENU_REMOVE,
        "移除盒子（保留文件夹）",
    )?;

    let action = match track_popup_menu(window, menu.0, screen_point) {
        FENCE_MENU_NEW => Some(HostUserAction::CreateStorageBox),
        FENCE_MENU_NEW_MAPPED => Some(HostUserAction::CreateMappedBox),
        FENCE_MENU_NEW_FOLDER => {
            create_folder_in_fence(window)?;
            None
        }
        FENCE_MENU_REFRESH => {
            refresh_fence(window, true);
            None
        }
        FENCE_MENU_OPEN => {
            open_shell_path(window, &snapshot.directory)?;
            None
        }
        FENCE_MENU_RENAME => prompt_rename(
            window,
            "修改盒子名称",
            "盒子名称：",
            &snapshot.title,
            "盒子名称不能为空",
        )?
        .map(|title| title.trim().to_string())
        .and_then(|title| {
            (title != snapshot.title).then(|| HostUserAction::RenameFence {
                id: snapshot.id.clone(),
                title,
            })
        }),
        FENCE_MENU_UNDO_GEOMETRY => {
            let _ = undo_fence_geometry(window)?;
            None
        }
        FENCE_MENU_RESET_SIZE => Some(HostUserAction::ResetFenceSize {
            id: snapshot.id.clone(),
        }),
        FENCE_MENU_COLLAPSE => Some(HostUserAction::ToggleFenceCollapsed {
            id: snapshot.id.clone(),
        }),
        FENCE_MENU_LOCK => Some(HostUserAction::ToggleFenceLocked {
            id: snapshot.id.clone(),
        }),
        command
            if command >= FENCE_MENU_COLOR_FIRST
                && command < FENCE_MENU_COLOR_FIRST + colors.len() =>
        {
            Some(HostUserAction::SetFenceColor {
                id: snapshot.id.clone(),
                color: colors[command - FENCE_MENU_COLOR_FIRST].0.into(),
            })
        }
        FENCE_MENU_REMOVE if confirm_remove_fence(window, &snapshot.title) => {
            Some(HostUserAction::RemoveFence {
                id: snapshot.id.clone(),
            })
        }
        _ => None,
    };
    if let Some(action) = action {
        emit_event(&HostEvent::UserAction { action });
    }
    Ok(())
}

fn fence_colors() -> &'static [(&'static str, &'static str)] {
    &[
        ("coral", "珊瑚"),
        ("sage", "鼠尾草绿"),
        ("butter", "奶油黄"),
        ("sky", "天空蓝"),
        ("lilac", "丁香紫"),
        ("graphite", "石墨灰"),
    ]
}

unsafe fn append_menu_item(
    menu: HMENU,
    flags: windows::Win32::UI::WindowsAndMessaging::MENU_ITEM_FLAGS,
    id: usize,
    label: &str,
) -> Result<(), String> {
    let label: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
    AppendMenuW(menu, flags, id, PCWSTR(label.as_ptr())).map_err(display_windows_error)
}

unsafe fn append_menu_separator(menu: HMENU) -> Result<(), String> {
    AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()).map_err(display_windows_error)
}

unsafe fn track_popup_menu(window: HWND, menu: HMENU, point: POINT) -> usize {
    let _ = SetForegroundWindow(window);
    let command = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        point.x,
        point.y,
        None,
        window,
        None,
    )
    .0 as usize;
    let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
    command
}

unsafe fn confirm_remove_fence(window: HWND, title: &str) -> bool {
    let message =
        format!("要从 DCreel 中移除「{title}」吗？\n\n磁盘上的文件夹和其中内容不会删除。");
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = "移除盒子".encode_utf16().chain(Some(0)).collect();
    MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        PCWSTR(caption.as_ptr()),
        MB_OKCANCEL | MB_ICONQUESTION,
    ) == IDOK
}

fn reveal_shell_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err("文件或文件夹已经不存在".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

unsafe fn prompt_rename(
    window: HWND,
    caption: &str,
    label: &str,
    current_title: &str,
    empty_message: &str,
) -> Result<Option<String>, String> {
    let mut parent_rect = RECT::default();
    GetWindowRect(window, &mut parent_rect).map_err(display_windows_error)?;
    let width = 380;
    let height = 168;
    let x = parent_rect.left + ((parent_rect.right - parent_rect.left - width) / 2).max(0);
    let y = parent_rect.top + ((parent_rect.bottom - parent_rect.top - height) / 2).max(0);
    let module = GetModuleHandleW(None).map_err(display_windows_error)?;
    let instance = HINSTANCE(module.0);
    let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(Some(0)).collect();
    let caption: Vec<u16> = caption.encode_utf16().chain(Some(0)).collect();
    let mut result: Option<Option<String>> = None;
    let state = Box::new(WindowState::RenameDialog(RenameDialogState {
        result: &mut result,
        edit: None,
        empty_message: empty_message.into(),
    }));
    let raw_state = Box::into_raw(state);
    let dialog = match CreateWindowExW(
        WS_EX_TOOLWINDOW,
        PCWSTR(class_name.as_ptr()),
        PCWSTR(caption.as_ptr()),
        WS_POPUP | WS_CAPTION | WS_SYSMENU,
        x,
        y,
        width,
        height,
        Some(window),
        None,
        Some(instance),
        Some(raw_state.cast::<c_void>()),
    ) {
        Ok(dialog) => dialog,
        Err(error) => {
            drop(Box::from_raw(raw_state));
            return Err(display_windows_error(error));
        }
    };

    let controls = (|| -> Result<(HWND, Vec<HWND>), String> {
        let label = create_dialog_control(
            "STATIC",
            label,
            WS_CHILD | WS_VISIBLE,
            18,
            18,
            330,
            20,
            dialog,
            0,
            instance,
        )?;
        let edit = create_dialog_control(
            "EDIT",
            current_title,
            WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            18,
            42,
            338,
            25,
            dialog,
            10,
            instance,
        )?;
        let confirm = create_dialog_control(
            "BUTTON",
            "确定",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            190,
            88,
            78,
            28,
            dialog,
            RENAME_DIALOG_OK,
            instance,
        )?;
        let cancel = create_dialog_control(
            "BUTTON",
            "取消",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            278,
            88,
            78,
            28,
            dialog,
            RENAME_DIALOG_CANCEL,
            instance,
        )?;
        Ok((edit, vec![label, edit, confirm, cancel]))
    })();
    let (edit, controls) = match controls {
        Ok(controls) => controls,
        Err(error) => {
            let _ = DestroyWindow(dialog);
            return Err(error);
        }
    };
    if let Some(WindowState::RenameDialog(state)) = state_mut(dialog) {
        state.edit = Some(edit);
    }
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in controls {
        let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    let _ = EnableWindow(window, false);
    let _ = ShowWindow(dialog, SW_SHOW);
    let _ = SetForegroundWindow(dialog);
    let _ = SetFocus(Some(edit));
    let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
        edit,
        EM_SETSEL,
        Some(WPARAM(0)),
        Some(LPARAM(-1)),
    );

    let mut loop_error = None;
    let mut message = MSG::default();
    while result.is_none() {
        let status = GetMessageW(&mut message, None, 0, 0);
        if status.0 == -1 {
            loop_error = Some(display_windows_error(Error::from_thread()));
            break;
        }
        if !status.as_bool() {
            PostQuitMessage(message.wParam.0 as i32);
            result = Some(None);
            break;
        }
        if !IsDialogMessageW(dialog, &message).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    let _ = EnableWindow(window, true);
    let _ = SetForegroundWindow(window);
    let _ = DestroyWindow(dialog);
    if let Some(error) = loop_error {
        Err(error)
    } else {
        Ok(result.flatten())
    }
}

// CreateWindowExW exposes every geometry and identity field separately. This
// small wrapper deliberately mirrors that API instead of hiding values in a tuple.
#[allow(clippy::too_many_arguments)]
unsafe fn create_dialog_control(
    class_name: &str,
    text: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    parent: HWND,
    id: u16,
    instance: HINSTANCE,
) -> Result<HWND, String> {
    let class_name: Vec<u16> = class_name.encode_utf16().chain(Some(0)).collect();
    let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    CreateWindowExW(
        Default::default(),
        PCWSTR(class_name.as_ptr()),
        PCWSTR(text.as_ptr()),
        style,
        x,
        y,
        width,
        height,
        Some(parent),
        (id != 0).then_some(HMENU(id as usize as *mut c_void)),
        Some(instance),
        None,
    )
    .map_err(display_windows_error)
}

unsafe fn handle_rename_dialog_command(window: HWND, command: u16) -> bool {
    match command {
        RENAME_DIALOG_CANCEL => {
            finish_rename_dialog(window, None);
            true
        }
        RENAME_DIALOG_OK => {
            let edit = match state_mut(window) {
                Some(WindowState::RenameDialog(state)) => state.edit,
                _ => None,
            };
            let Some(edit) = edit else {
                finish_rename_dialog(window, None);
                return true;
            };
            let length = GetWindowTextLengthW(edit).max(0) as usize;
            let mut text = vec![0_u16; length + 1];
            let copied = GetWindowTextW(edit, &mut text).max(0) as usize;
            let title = String::from_utf16_lossy(&text[..copied]);
            if title.trim().is_empty() {
                let empty_message = match state_mut(window) {
                    Some(WindowState::RenameDialog(state)) => state.empty_message.clone(),
                    _ => "名称不能为空".into(),
                };
                let message: Vec<u16> = empty_message.encode_utf16().chain(Some(0)).collect();
                let caption: Vec<u16> = "DCreel".encode_utf16().chain(Some(0)).collect();
                let _ = MessageBoxW(
                    Some(window),
                    PCWSTR(message.as_ptr()),
                    PCWSTR(caption.as_ptr()),
                    MB_OK,
                );
                let _ = SetFocus(Some(edit));
            } else {
                finish_rename_dialog(window, Some(title));
            }
            true
        }
        _ => false,
    }
}

unsafe fn finish_rename_dialog(window: HWND, value: Option<String>) {
    if let Some(WindowState::RenameDialog(state)) = state_mut(window)
        && !state.result.is_null()
    {
        *state.result = Some(value);
    }
    let _ = DestroyWindow(window);
}

unsafe fn open_fence_item_at(window: HWND, point: POINT) -> bool {
    let mut client = RECT::default();
    if GetClientRect(window, &mut client).is_err() {
        return false;
    }
    let path = match state_mut(window) {
        Some(WindowState::Fence(state))
            if !effectively_collapsed(&state.snapshot, state.preferences.show_fence_titles) =>
        {
            item_index_at(
                &client,
                content_top(state.preferences.show_fence_titles),
                state.preferences.icon_size,
                state.items.len(),
                state.scroll_row,
                point,
            )
            .and_then(|index| state.items.get(index))
            .map(|item| item.path.clone())
        }
        _ => None,
    };
    let Some(path) = path else {
        return false;
    };
    if let Err(error) = open_shell_path(window, &path) {
        emit_event(&HostEvent::Notification {
            message: format!("无法打开 {}：{error}", path.display()),
        });
    }
    true
}

unsafe fn open_shell_path(window: HWND, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err("文件或文件夹已经不存在".into());
    }
    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = ShellExecuteW(
        Some(window),
        PCWSTR(operation.as_ptr()),
        PCWSTR(target.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
    let code = result.0 as isize;
    if code <= 32 {
        Err(format!("Windows Shell 返回错误码 {code}"))
    } else {
        Ok(())
    }
}

fn thumbnail_candidate(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "jpe"
            | "gif"
            | "webp"
            | "bmp"
            | "tif"
            | "tiff"
            | "heic"
            | "heif"
            | "avif"
            | "ico"
            | "svg"
            | "psd"
            | "raw"
            | "dng"
            | "mp4"
            | "m4v"
            | "mov"
            | "mkv"
            | "avi"
            | "wmv"
            | "webm"
            | "mpg"
            | "mpeg"
            | "mp3"
            | "m4a"
            | "flac"
            | "wav"
            | "wma"
            | "ogg"
            | "pdf"
    )
}

fn extract_shell_visual(path: &Path, size: u32) -> Option<CachedBitmap> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
    let requested_size = SIZE {
        cx: size as i32,
        cy: size as i32,
    };
    if path.is_file()
        && thumbnail_candidate(path)
        && let Some(thumbnail) = extract_factory_bitmap(
            &factory,
            requested_size,
            SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
            ShellVisualKind::Thumbnail,
        )
    {
        return Some(thumbnail);
    }
    extract_factory_bitmap(
        &factory,
        requested_size,
        SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        ShellVisualKind::Icon,
    )
}

fn extract_factory_bitmap(
    factory: &IShellItemImageFactory,
    requested_size: SIZE,
    flags: SIIGBF,
    kind: ShellVisualKind,
) -> Option<CachedBitmap> {
    let bitmap = unsafe { factory.GetImage(requested_size, flags) }.ok()?;
    if bitmap.0.is_null() {
        return None;
    }
    let mut details = BITMAP::default();
    let copied = unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut details as *mut BITMAP).cast::<c_void>()),
        )
    };
    if copied == 0 || details.bmWidth <= 0 || details.bmHeight == 0 {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return None;
    }
    let width = details.bmWidth;
    let height = details.bmHeight.unsigned_abs();
    let (bitmap, uses_alpha) = normalize_shell_bitmap(bitmap, width, height);
    Some(CachedBitmap {
        handle: bitmap,
        width,
        height: height as i32,
        kind,
        uses_alpha,
    })
}

fn normalize_shell_bitmap(bitmap: HBITMAP, width: i32, height: u32) -> (HBITMAP, bool) {
    let Some(mut pixels) = read_bitmap_pixels(bitmap, width, height) else {
        return (bitmap, false);
    };
    let uses_alpha = normalize_shell_bgra(&mut pixels);
    if !uses_alpha {
        return (bitmap, false);
    }
    let Some(normalized) = create_bgra_bitmap(width, height, &pixels) else {
        return (bitmap, true);
    };
    unsafe {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    (normalized, true)
}

fn read_bitmap_pixels(bitmap: HBITMAP, width: i32, height: u32) -> Option<Vec<u8>> {
    let Ok(width) = u32::try_from(width) else {
        return None;
    };
    let Ok(height_i32) = i32::try_from(height) else {
        return None;
    };
    let byte_count = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())?;
    if byte_count > 64 * 1024 * 1024 {
        return None;
    }
    let Ok(image_size) = u32::try_from(byte_count) else {
        return None;
    };
    let mut pixels = vec![0_u8; byte_count];
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -height_i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: image_size,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return None;
    }
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast::<c_void>()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(dc);
    }
    (lines == height_i32).then_some(pixels)
}

fn normalize_shell_bgra(pixels: &mut [u8]) -> bool {
    let pixels = pixels.as_chunks_mut::<4>().0;
    let uses_alpha = pixels.iter().any(|pixel| pixel[3] != 0);
    if !uses_alpha {
        return false;
    }
    // IShellItemImageFactory 返回的 HBITMAP 在不同图标处理器之间并不完全
    // 一致：有些是预乘 Alpha，有些是直通 Alpha，还有一些会在 alpha=0 的
    // 像素里留下未初始化的 RGB。AlphaBlend 要求预乘 Alpha，后两种情况会把
    // 透明边缘画成青色、红色或黑色细条。
    let straight_alpha = pixels.iter().any(|pixel| {
        let alpha = pixel[3];
        alpha > 0 && alpha < 255 && pixel[..3].iter().any(|channel| *channel > alpha)
    });
    for pixel in pixels {
        let alpha = pixel[3];
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 && straight_alpha {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * u16::from(alpha) + 127) / 255) as u8;
            }
        }
    }
    true
}

fn create_bgra_bitmap(width: i32, height: u32, pixels: &[u8]) -> Option<HBITMAP> {
    let height_i32 = i32::try_from(height).ok()?;
    let expected = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if pixels.len() != expected {
        return None;
    }
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height_i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: u32::try_from(expected).ok()?,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut::<c_void>();
    let bitmap =
        unsafe { CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return None;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
    }
    Some(bitmap)
}

unsafe fn draw_visual_overlay(
    destination: windows::Win32::Graphics::Gdi::HDC,
    visual: &DesktopVisualOverlay,
) -> bool {
    let bounds = &visual.bounds;
    let available_width = bounds.right - bounds.left;
    let available_height = bounds.bottom - bounds.top;
    if available_width <= 0 || available_height <= 0 || visual.width <= 0 || visual.height <= 0 {
        return false;
    }
    let (draw_width, draw_height) =
        if available_width * visual.height <= available_height * visual.width {
            (
                available_width,
                (visual.height * available_width / visual.width).max(1),
            )
        } else {
            (
                (visual.width * available_height / visual.height).max(1),
                available_height,
            )
        };
    let draw_x = bounds.left + (available_width - draw_width) / 2;
    let draw_y = bounds.top + (available_height - draw_height) / 2;
    if visual.kind == ShellVisualKind::Thumbnail {
        let background = CreateSolidBrush(rgb(255, 255, 255));
        FillRect(destination, bounds, background);
        let _ = DeleteObject(HGDIOBJ(background.0));
    }
    let source = CreateCompatibleDC(Some(destination));
    if source.0.is_null() {
        return false;
    }
    let previous = SelectObject(source, HGDIOBJ(visual.handle.0));
    let result = if visual.uses_alpha {
        AlphaBlend(
            destination,
            draw_x,
            draw_y,
            draw_width,
            draw_height,
            source,
            0,
            0,
            visual.width,
            visual.height,
            BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            },
        )
        .as_bool()
    } else {
        StretchBlt(
            destination,
            draw_x,
            draw_y,
            draw_width,
            draw_height,
            Some(source),
            0,
            0,
            visual.width,
            visual.height,
            SRCCOPY,
        )
        .as_bool()
    };
    let _ = SelectObject(source, previous);
    let _ = DeleteDC(source);
    result
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        unsafe { OleInitialize(None) }
            .map(|_| Self)
            .map_err(|error| format!("无法初始化 Desktop Host OLE：{error}"))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

fn list_folder(folder: &Path, show_hidden_files: bool) -> Vec<FolderItem> {
    let mut items = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else {
        return items;
    };
    for entry in entries.flatten().take(MAX_FOLDER_ITEMS) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let metadata = entry.metadata().ok();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        if !show_hidden_files
            && (name.starts_with('.')
                || metadata
                    .as_ref()
                    .is_some_and(|value| value.file_attributes() & 0x2 != 0))
        {
            continue;
        }
        items.push(FolderItem {
            name,
            path: entry.path(),
            is_dir: file_type.is_dir(),
            modified_at_nanos: metadata
                .as_ref()
                .and_then(|value| value.modified().ok())
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_nanos())
                .unwrap_or_default(),
            length: metadata
                .as_ref()
                .map(|value| value.len())
                .unwrap_or_default(),
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

unsafe fn activate_fence_temporarily(window: HWND) {
    if let Some(WindowState::Fence(state)) = state_mut(window) {
        state.foreground_until = Some(Instant::now() + FOREGROUND_IDLE_TIMEOUT);
    }
    let _ = SetWindowPos(
        window,
        Some(HWND_TOP),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER,
    );
    let _ = SetForegroundWindow(window);
}

unsafe fn extend_fence_foreground_activity(window: HWND) {
    if let Some(WindowState::Fence(state)) = state_mut(window)
        && state.foreground_until.is_some()
    {
        state.foreground_until = Some(Instant::now() + FOREGROUND_IDLE_TIMEOUT);
    }
}

unsafe fn should_refresh_fence_layers(window: HWND) -> bool {
    match state_mut(window) {
        Some(WindowState::Fence(state)) => {
            let foreground_pending = state.foreground_until.is_some();
            if foreground_pending
                || state.last_layer_refresh.elapsed() >= DESKTOP_LAYER_WATCHDOG_INTERVAL
            {
                state.last_layer_refresh = Instant::now();
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

/// 平时保持盒子在 Explorer 桌面宿主上方、普通应用窗口下方；用户点击后
/// 临时提升到普通窗口前方，停止操作一段时间后再自动回落。
unsafe fn place_fence_layers(fence_window: HWND) -> Result<(), String> {
    let keep_foreground = match state_mut(fence_window) {
        Some(WindowState::Fence(state)) => match state.foreground_until {
            Some(deadline) if deadline > Instant::now() => true,
            Some(_) => {
                state.foreground_until = None;
                false
            }
            None => false,
        },
        _ => false,
    };
    if keep_foreground {
        return SetWindowPos(
            fence_window,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        )
        .map_err(display_windows_error);
    }
    place_on_desktop(fence_window)
}

fn title_bar_height(show_fence_titles: bool) -> i32 {
    if show_fence_titles {
        HEADER_HEIGHT
    } else {
        COMPACT_HEADER_HEIGHT
    }
}

fn hidden_title_offset(show_fence_titles: bool) -> i32 {
    HEADER_HEIGHT - title_bar_height(show_fence_titles)
}

fn content_top(show_fence_titles: bool) -> i32 {
    title_bar_height(show_fence_titles) + CONTENT_TOP_PADDING
}

fn effectively_collapsed(snapshot: &HostFenceSnapshot, show_fence_titles: bool) -> bool {
    snapshot.collapsed && show_fence_titles
}

fn window_y(logical_y: f64, show_fence_titles: bool) -> i32 {
    logical_i32(logical_y).saturating_add(hidden_title_offset(show_fence_titles))
}

fn visible_height(snapshot: &HostFenceSnapshot, show_fence_titles: bool) -> i32 {
    if effectively_collapsed(snapshot, show_fence_titles) {
        HEADER_HEIGHT
    } else {
        logical_i32(snapshot.height.max(f64::from(MIN_FENCE_HEIGHT)))
            .saturating_sub(hidden_title_offset(show_fence_titles))
            .max(1)
    }
}

fn logical_i32(value: f64) -> i32 {
    value.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

const fn rgb(red: u32, green: u32, blue: u32) -> COLORREF {
    COLORREF(red | (green << 8) | (blue << 16))
}

fn color_for_name(name: &str) -> COLORREF {
    match name {
        "sage" => rgb(106, 148, 119),
        "butter" => rgb(211, 168, 72),
        "sky" => rgb(91, 144, 184),
        "lilac" => rgb(144, 116, 174),
        "graphite" => rgb(84, 84, 88),
        "coral" => rgb(226, 119, 99),
        _ => parse_hex_color(name).unwrap_or_else(|| rgb(226, 119, 99)),
    }
}

fn content_color_for_name(name: &str) -> COLORREF {
    match name {
        "paper" => rgb(245, 239, 229),
        "frosted" => rgb(229, 238, 241),
        _ => color_for_name(name),
    }
}

fn parse_hex_color(value: &str) -> Option<COLORREF> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return None;
    }
    let byte = |high: u8, low: u8| -> Option<u32> {
        Some(u32::from(hex_nibble(high)?) * 16 + u32::from(hex_nibble(low)?))
    };
    Some(rgb(
        byte(bytes[1], bytes[2])?,
        byte(bytes[3], bytes[4])?,
        byte(bytes[5], bytes[6])?,
    ))
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn emit_event(event: &HostEvent) {
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    if serde_json::to_writer(&mut output, event).is_ok() {
        let _ = output.write_all(b"\n");
        let _ = output.flush();
    }
}

fn display_windows_error(error: Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_display(
        name: &str,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        dpi: u32,
    ) -> DisplayAnchor {
        DisplayAnchor {
            device_name: name.into(),
            work_left: left,
            work_top: top,
            work_width: width,
            work_height: height,
            dpi_x: dpi,
            dpi_y: dpi,
            effective_dpi_x: Some(dpi),
            effective_dpi_y: Some(dpi),
        }
    }

    fn test_fence(
        id: &str,
        x: f64,
        y: f64,
        width: f64,
        anchor: DisplayAnchor,
    ) -> HostFenceSnapshot {
        HostFenceSnapshot {
            id: id.into(),
            title: id.into(),
            directory: PathBuf::from(format!(r"C:\layout-test\{id}")),
            x,
            y,
            width,
            height: 280.0,
            color: "sage".into(),
            content_color: "paper".into(),
            collapsed: false,
            locked: false,
            display_anchor: Some(anchor),
            placement: None,
        }
    }

    fn selection_items() -> Vec<FolderItem> {
        ["a.txt", "b.txt", "c.txt", "d.txt"]
            .into_iter()
            .map(|name| FolderItem {
                name: name.into(),
                path: PathBuf::from(format!(r"C:\selection-test\{name}")),
                is_dir: false,
                modified_at_nanos: 0,
                length: 0,
            })
            .collect()
    }

    #[test]
    fn moving_preserves_size_and_supports_negative_coordinates() {
        let result = interaction_rect(
            InteractionMode::Move,
            POINT { x: 100, y: 100 },
            RECT {
                left: 20,
                top: 30,
                right: 350,
                bottom: 310,
            },
            POINT { x: -40, y: 130 },
            MIN_FENCE_HEIGHT,
            MAX_FENCE_HEIGHT,
        );
        assert_eq!(result.left, -120);
        assert_eq!(result.top, 60);
        assert_eq!(result.right - result.left, 330);
        assert_eq!(result.bottom - result.top, 280);
    }

    #[test]
    fn display_layout_keeps_a_right_top_group_right_top_after_resolution_change() {
        let source = test_display(r"\\.\DISPLAY1", 0, 0, 1_920, 1_040, 96);
        let target = test_display(r"\\.\DISPLAY1", 0, 0, 2_560, 1_400, 96);
        let fence = test_fence("right-top", 1_600.0, 0.0, 320.0, source);

        let (mapped, events) =
            reconcile_display_layout(vec![fence], &[(target.clone(), true)], true);

        assert_eq!(mapped[0].x, 2_240.0);
        assert_eq!(mapped[0].y, 0.0);
        assert_eq!(mapped[0].display_anchor.as_ref(), Some(&target));
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn persisted_group_anchor_survives_resolution_and_windows_scale_change() {
        let source = test_display(r"\\.\DISPLAY1", 0, 0, 2_560, 1_540, 120);
        let target = test_display(r"\\.\DISPLAY1", 0, 0, 2_880, 1_740, 144);
        let mut fences = vec![
            test_fence("left", 1_900.0, 0.0, 330.0, source.clone()),
            test_fence("right", 2_230.0, 0.0, 330.0, source),
        ];
        assign_layout_placements(&mut fences, true);

        let left_placement = fences[0].placement.as_ref().unwrap();
        let right_placement = fences[1].placement.as_ref().unwrap();
        assert_eq!(left_placement.group_id, right_placement.group_id);
        assert_eq!(left_placement.horizontal.anchor, LayoutAnchor::End);
        assert_eq!(left_placement.vertical.anchor, LayoutAnchor::Start);

        let (mapped, events) = reconcile_display_layout(fences, &[(target.clone(), true)], true);
        let left = snapshot_window_rect(&mapped[0], true);
        let right = snapshot_window_rect(&mapped[1], true);

        assert_eq!(left.right, right.left);
        assert_eq!(right.right, display_work_rect(&target).right);
        assert_eq!(left.top, display_work_rect(&target).top);
        assert_eq!(right.top, display_work_rect(&target).top);
        assert_eq!(left.right - left.left, 396);
        assert_eq!(right.right - right.left, 396);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn free_group_uses_the_movable_area_ratio() {
        let source = test_display(r"\\.\DISPLAY1", 0, 0, 1_600, 900, 96);
        let target = test_display(r"\\.\DISPLAY1", 0, 0, 2_400, 1_350, 96);
        let mut fences = vec![test_fence("middle", 640.0, 310.0, 320.0, source)];
        assign_layout_placements(&mut fences, true);
        let placement = fences[0].placement.as_ref().unwrap();
        assert_eq!(placement.horizontal.anchor, LayoutAnchor::Proportional);
        assert_eq!(placement.vertical.anchor, LayoutAnchor::Proportional);

        let (mapped, _) = reconcile_display_layout(fences, &[(target, true)], true);
        assert_eq!(mapped[0].x, 1_040.0);
        assert_eq!(mapped[0].y, 535.0);
    }

    #[test]
    fn display_layout_preserves_touching_edges_and_scales_a_visual_gap_by_dpi() {
        let source = test_display(r"\\.\DISPLAY1", 0, 0, 1_600, 900, 96);
        let target = test_display(r"\\.\DISPLAY1", 0, 0, 2_560, 1_400, 144);
        let right = test_fence("right", 1_300.0, 0.0, 300.0, source.clone());
        let middle = test_fence("middle", 1_000.0, 0.0, 300.0, source.clone());
        let left = test_fence("left", 660.0, 0.0, 300.0, source);

        let (mapped, _) =
            reconcile_display_layout(vec![right, middle, left], &[(target, true)], true);
        let right_rect = snapshot_window_rect(&mapped[0], true);
        let middle_rect = snapshot_window_rect(&mapped[1], true);
        let left_rect = snapshot_window_rect(&mapped[2], true);

        assert_eq!(middle_rect.right, right_rect.left);
        assert_eq!(middle_rect.left - left_rect.right, 60);
        assert_eq!(right_rect.right, 2_560);
        assert!(!rects_intersect(&right_rect, &middle_rect));
        assert!(!rects_intersect(&middle_rect, &left_rect));
    }

    #[test]
    fn display_layout_falls_back_to_the_primary_monitor_when_a_display_is_removed() {
        let removed = test_display(r"\\.\DISPLAY2", 1_920, 0, 1_920, 1_040, 96);
        let primary = test_display(r"\\.\DISPLAY1", 0, 0, 1_920, 1_040, 96);
        let fence = test_fence("removed-display", 3_200.0, 120.0, 320.0, removed);

        let (mapped, _) = reconcile_display_layout(vec![fence], &[(primary.clone(), true)], true);
        let rect = snapshot_window_rect(&mapped[0], true);

        assert!(rect_inside(&rect, &display_work_rect(&primary)));
        assert_eq!(mapped[0].display_anchor.as_ref(), Some(&primary));
    }

    #[test]
    fn display_layout_avoids_overlap_when_dpi_scaling_hits_the_minimum_width() {
        let source = test_display(r"\\.\DISPLAY1", 0, 0, 1_600, 900, 144);
        let target = test_display(r"\\.\DISPLAY1", 0, 0, 700, 600, 96);
        let right = test_fence("right", 1_300.0, 0.0, 300.0, source.clone());
        let left = test_fence("left", 1_000.0, 0.0, 300.0, source);

        let (mapped, _) =
            reconcile_display_layout(vec![right, left], &[(target.clone(), true)], true);
        let right_rect = snapshot_window_rect(&mapped[0], true);
        let left_rect = snapshot_window_rect(&mapped[1], true);

        assert!(rect_inside(&right_rect, &display_work_rect(&target)));
        assert!(rect_inside(&left_rect, &display_work_rect(&target)));
        assert_eq!(left_rect.right, right_rect.left);
        assert!(!rects_intersect(&right_rect, &left_rect));
    }

    #[test]
    fn corner_resize_obeys_minimum_and_maximum_size() {
        let edges = ResizeEdges {
            right: true,
            bottom: true,
            ..ResizeEdges::default()
        };
        let start = RECT {
            left: 40,
            top: 50,
            right: 370,
            bottom: 330,
        };
        let minimum = interaction_rect(
            InteractionMode::Resize(edges),
            POINT { x: 0, y: 0 },
            start,
            POINT {
                x: -4_000,
                y: -4_000,
            },
            MIN_FENCE_HEIGHT,
            MAX_FENCE_HEIGHT,
        );
        assert_eq!(minimum.right - minimum.left, MIN_FENCE_WIDTH);
        assert_eq!(minimum.bottom - minimum.top, MIN_FENCE_HEIGHT);

        let maximum = interaction_rect(
            InteractionMode::Resize(edges),
            POINT { x: 0, y: 0 },
            start,
            POINT { x: 4_000, y: 4_000 },
            MIN_FENCE_HEIGHT,
            MAX_FENCE_HEIGHT,
        );
        assert_eq!(maximum.right - maximum.left, MAX_FENCE_WIDTH);
        assert_eq!(maximum.bottom - maximum.top, MAX_FENCE_HEIGHT);
    }

    #[test]
    fn moving_and_resizing_snap_to_sibling_edges_and_size() {
        let sibling = RECT {
            left: 500,
            top: 100,
            right: 830,
            bottom: 380,
        };
        let moved = snap_interaction_rect(
            InteractionMode::Move,
            RECT {
                left: 823,
                top: 108,
                right: 1153,
                bottom: 388,
            },
            &[sibling],
            MIN_FENCE_HEIGHT,
            MAX_FENCE_HEIGHT,
        );
        assert_eq!(moved.left, sibling.right);
        assert_eq!(moved.top, sibling.top);
        assert!(!rects_intersect(&moved, &sibling));

        let resized = snap_interaction_rect(
            InteractionMode::Resize(ResizeEdges {
                right: true,
                bottom: true,
                ..ResizeEdges::default()
            }),
            RECT {
                left: 100,
                top: 100,
                right: 421,
                bottom: 371,
            },
            &[sibling],
            MIN_FENCE_HEIGHT,
            MAX_FENCE_HEIGHT,
        );
        assert_eq!(resized.right - resized.left, sibling.right - sibling.left);
        assert_eq!(resized.bottom - resized.top, sibling.bottom - sibling.top);
    }

    #[test]
    fn moving_and_resizing_stop_at_sibling_edges_instead_of_overlapping() {
        let sibling = RECT {
            left: 500,
            top: 100,
            right: 830,
            bottom: 380,
        };
        let move_start = RECT {
            left: 100,
            top: 100,
            right: 430,
            bottom: 380,
        };
        let moved = prevent_sibling_overlap(
            InteractionMode::Move,
            move_start,
            RECT {
                left: 400,
                top: 100,
                right: 730,
                bottom: 380,
            },
            &[sibling],
            MIN_FENCE_HEIGHT,
            MAX_FENCE_HEIGHT,
        );
        assert_eq!(moved.right, sibling.left);
        assert!(!rects_intersect(&moved, &sibling));

        let resized = prevent_sibling_overlap(
            InteractionMode::Resize(ResizeEdges {
                right: true,
                ..ResizeEdges::default()
            }),
            move_start,
            RECT {
                right: 650,
                ..move_start
            },
            &[sibling],
            MIN_FENCE_HEIGHT,
            MAX_FENCE_HEIGHT,
        );
        assert_eq!(resized.right, sibling.left);
        assert!(!rects_intersect(&resized, &sibling));
    }

    #[test]
    fn ghost_hotkey_parser_supports_multiple_ordinary_keys_and_function_keys() {
        let shortcut = parse_hotkey("Z+X").unwrap();
        assert_eq!(shortcut.keys, vec![u32::from(b'X'), u32::from(b'Z')]);
        assert_eq!(
            parse_hotkey("Ctrl+Alt+G").unwrap().keys,
            vec![0x11, 0x12, u32::from(b'G')]
        );
        assert_eq!(parse_hotkey("Shift+F12").unwrap().keys, vec![0x10, 0x7b]);
        assert_eq!(parse_hotkey("G").unwrap().keys, vec![u32::from(b'G')]);
        assert!(parse_hotkey("Ctrl+Shift").is_err());
        assert!(parse_hotkey("Z+Z").is_err());
        assert!(parse_hotkey("Ctrl+unknown-key").is_err());
    }

    #[test]
    fn ghost_hotkey_tracker_triggers_once_until_every_chord_key_is_released() {
        let mut tracker = HotkeyChordTracker::default();
        tracker.set_chord(Some(parse_hotkey("Z+X").unwrap()));

        assert_eq!(
            tracker.handle_key(u32::from(b'Z'), true),
            HotkeyHookResult {
                trigger: false,
                suppress: true,
                ..HotkeyHookResult::default()
            }
        );
        assert_eq!(
            tracker.handle_key(u32::from(b'X'), true),
            HotkeyHookResult {
                trigger: true,
                suppress: true,
                ..HotkeyHookResult::default()
            }
        );
        assert!(!tracker.handle_key(u32::from(b'X'), true).trigger);
        assert!(tracker.handle_key(u32::from(b'X'), false).suppress);
        assert!(!tracker.handle_key(u32::from(b'X'), true).trigger);
        assert!(tracker.handle_key(u32::from(b'X'), false).suppress);
        assert!(tracker.handle_key(u32::from(b'Z'), false).suppress);

        assert!(!tracker.handle_key(u32::from(b'X'), true).trigger);
        assert!(tracker.handle_key(u32::from(b'Z'), true).trigger);
    }

    #[test]
    fn ordinary_hotkey_members_are_replayed_when_the_chord_is_not_completed() {
        let mut tracker = HotkeyChordTracker::default();
        tracker.set_chord(Some(parse_hotkey("Z+X").unwrap()));

        let down = tracker.handle_key(u32::from(b'Z'), true);
        assert!(down.suppress);
        assert!(down.replay.is_empty());

        let up = tracker.handle_key(u32::from(b'Z'), false);
        assert!(up.suppress);
        assert_eq!(
            up.replay,
            vec![
                ReplayKeyboardEvent::key(u32::from(b'Z'), true),
                ReplayKeyboardEvent::key(u32::from(b'Z'), false),
            ]
        );

        let x_down = tracker.handle_key(u32::from(b'X'), true);
        assert!(x_down.suppress);
        let x_up = tracker.handle_key(u32::from(b'X'), false);
        assert_eq!(
            x_up.replay,
            vec![
                ReplayKeyboardEvent::key(u32::from(b'X'), true),
                ReplayKeyboardEvent::key(u32::from(b'X'), false),
            ]
        );
    }

    #[test]
    fn overlapping_normal_typing_is_replayed_in_input_order() {
        let mut tracker = HotkeyChordTracker::default();
        tracker.set_chord(Some(parse_hotkey("Z+X").unwrap()));

        assert!(tracker.handle_key(u32::from(b'Z'), true).suppress);
        let unrelated = tracker.handle_key(u32::from(b'A'), true);
        assert!(unrelated.suppress);
        assert_eq!(
            unrelated.replay,
            vec![
                ReplayKeyboardEvent::key(u32::from(b'Z'), true),
                ReplayKeyboardEvent::key(u32::from(b'Z'), false),
                ReplayKeyboardEvent::key(u32::from(b'A'), true),
            ]
        );
        assert_eq!(
            tracker.handle_key(u32::from(b'A'), false),
            HotkeyHookResult::default()
        );
        assert!(tracker.handle_key(u32::from(b'Z'), false).suppress);
    }

    #[test]
    fn incomplete_modified_multi_key_chord_replays_the_original_shortcut() {
        let mut tracker = HotkeyChordTracker::default();
        tracker.set_chord(Some(parse_hotkey("Ctrl+Z+X").unwrap()));

        assert_eq!(tracker.handle_key(0xa2, true), HotkeyHookResult::default());
        assert!(tracker.handle_key(u32::from(b'Z'), true).suppress);
        let z_up = tracker.handle_key(u32::from(b'Z'), false);
        assert_eq!(
            z_up.replay,
            vec![
                ReplayKeyboardEvent::key(u32::from(b'Z'), true),
                ReplayKeyboardEvent::key(u32::from(b'Z'), false),
            ]
        );
        assert!(z_up.suppress);
        assert_eq!(tracker.handle_key(0xa2, false), HotkeyHookResult::default());
    }

    #[test]
    fn modified_ghost_hotkey_suppresses_the_action_key_but_not_modifiers() {
        let mut tracker = HotkeyChordTracker::default();
        tracker.set_chord(Some(parse_hotkey("Ctrl+Shift+Z").unwrap()));

        assert_eq!(tracker.handle_key(0xa2, true), HotkeyHookResult::default());
        assert_eq!(tracker.handle_key(0xa1, true), HotkeyHookResult::default());
        assert_eq!(
            tracker.handle_key(u32::from(b'Z'), true),
            HotkeyHookResult {
                trigger: true,
                suppress: true,
                ..HotkeyHookResult::default()
            }
        );
        assert!(tracker.handle_key(u32::from(b'Z'), false).suppress);
        assert!(!tracker.handle_key(0xa1, false).suppress);
        assert!(!tracker.handle_key(0xa2, false).suppress);
    }

    #[test]
    fn item_grid_and_hit_testing_share_the_same_cells() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 330,
            bottom: 285,
        };
        let cells = item_cells(&client, content_top(true), 46, 10, 0);
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[0].index, 0);
        assert_eq!(cells[3].index, 3);

        let target = cells[4].bounds;
        assert_eq!(
            item_index_at(
                &client,
                content_top(true),
                46,
                10,
                0,
                POINT {
                    x: target.left + 2,
                    y: target.top + 2,
                },
            ),
            Some(4)
        );
        assert_eq!(
            item_index_at(
                &client,
                content_top(true),
                46,
                10,
                0,
                POINT { x: 20, y: 20 },
            ),
            None
        );
    }

    #[test]
    fn item_grid_distributes_columns_across_the_complete_content_width() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 396,
            bottom: 285,
        };
        let metrics = grid_metrics(&client, content_top(true), 46, 3).unwrap();
        assert_eq!(metrics.columns, 3);

        let cells = item_cells(&client, content_top(true), 46, 3, 0);
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].bounds.left, CONTENT_LEFT);
        assert_eq!(cells[2].bounds.right, client.right - CONTENT_RIGHT);
        assert_eq!(cells[0].bounds.right, cells[1].bounds.left);
        assert_eq!(cells[1].bounds.right, cells[2].bounds.left);
        assert_eq!(cells[0].bounds.right - cells[0].bounds.left, 124);
        assert_eq!(cells[1].bounds.right - cells[1].bounds.left, 124);
        assert_eq!(cells[2].bounds.right - cells[2].bounds.left, 124);
    }

    #[test]
    fn file_name_metrics_scale_with_the_icon_size() {
        assert_eq!(scale_item_metric(44, 36), 34);
        assert_eq!(scale_item_metric(44, 46), 44);
        assert_eq!(scale_item_metric(44, 64), 61);
        assert_eq!(scale_desktop_font_height(-18, 36), -16);
        assert_eq!(scale_desktop_font_height(-18, 46), -18);
        assert_eq!(scale_desktop_font_height(-18, 64), -25);

        let client = RECT {
            left: 0,
            top: 0,
            right: 1_000,
            bottom: 600,
        };
        let small = item_cells(&client, content_top(false), 36, 2, 0);
        let normal = item_cells(&client, content_top(false), 46, 2, 0);
        assert!(
            small[0].label.bottom - small[0].label.top
                < normal[0].label.bottom - normal[0].label.top
        );
        assert!(
            small[0].label.top - small[0].icon.bottom < normal[0].label.top - normal[0].icon.bottom
        );
        assert!(
            small[1].bounds.left - small[0].bounds.left
                < normal[1].bounds.left - normal[0].bounds.left
        );
    }

    #[test]
    fn scrolled_grid_draws_and_hits_the_same_later_items() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 330,
            bottom: 285,
        };
        let metrics = grid_metrics(&client, content_top(true), 46, 30).unwrap();
        assert_eq!(metrics.columns, 3);
        assert_eq!(metrics.visible_rows, 2);
        assert_eq!(metrics.total_rows, 10);
        assert_eq!(metrics.max_scroll_row, 8);

        let cells = item_cells(&client, content_top(true), 46, 30, 4);
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[0].index, 12);
        assert_eq!(cells[5].index, 17);
        let target = cells[2].bounds;
        assert_eq!(
            item_index_at(
                &client,
                content_top(true),
                46,
                30,
                4,
                POINT {
                    x: target.left + 2,
                    y: target.top + 2,
                },
            ),
            Some(14)
        );
    }

    #[test]
    fn wheel_pages_keep_the_previous_page_last_row_visible() {
        for (visible_rows, expected_step) in [(2, 1), (3, 2), (5, 4)] {
            let metrics = GridMetrics {
                visible_rows,
                max_scroll_row: 20,
                ..GridMetrics::default()
            };
            assert_eq!(scroll_page_step(visible_rows), expected_step);
            assert_eq!(wheel_scroll_row(0, -1, metrics), expected_step);
            assert_eq!(wheel_scroll_row(expected_step, 1, metrics), 0);
        }
    }

    #[test]
    fn final_wheel_page_stays_on_page_boundary_and_leaves_empty_rows() {
        let content_top = content_top(true);
        let row_height = 46 + scale_item_metric(44, 46);
        let client = RECT {
            left: 0,
            top: 0,
            right: 330,
            bottom: content_top + row_height * 3 + CONTENT_BOTTOM,
        };
        let metrics = grid_metrics(&client, content_top, 46, 10).unwrap();
        assert_eq!(metrics.columns, 3);
        assert_eq!(metrics.visible_rows, 3);
        assert_eq!(metrics.total_rows, 4);
        assert_eq!(metrics.max_scroll_row, 2);
        assert_eq!(wheel_scroll_row(0, -1, metrics), 2);

        let cells = item_cells(&client, content_top, 46, 10, metrics.max_scroll_row);
        assert_eq!(
            cells.iter().map(|cell| cell.index).collect::<Vec<_>>(),
            vec![6, 7, 8, 9]
        );
        assert!(cells[..3].iter().all(|cell| cell.bounds.top == content_top));
        assert_eq!(cells[3].bounds.top, content_top + row_height);
        assert!(
            cells
                .iter()
                .all(|cell| cell.bounds.top < content_top + row_height * 2)
        );
    }

    #[test]
    fn wheel_delta_keeps_signed_high_word() {
        let up = WPARAM((120_u16 as usize) << 16);
        let down = WPARAM(((-120_i16) as u16 as usize) << 16);
        assert_eq!(wheel_delta(up), 120);
        assert_eq!(wheel_delta(down), -120);
    }

    #[test]
    fn packed_mouse_coordinates_keep_signed_values() {
        let x = -12_i16;
        let y = 34_i16;
        let packed = u32::from(x as u16) | (u32::from(y as u16) << 16);
        assert_eq!(
            client_point(LPARAM(packed as isize)),
            POINT {
                x: i32::from(x),
                y: i32::from(y),
            }
        );
    }

    #[test]
    fn missing_item_is_rejected_before_shell_execute() {
        let path = std::env::temp_dir().join("creel-host-definitely-missing-item.test");
        let error = unsafe { open_shell_path(HWND::default(), &path) }.unwrap_err();
        assert!(error.contains("不存在"));
    }

    #[test]
    fn item_drag_waits_until_the_pointer_crosses_the_threshold() {
        let start = POINT { x: 100, y: 100 };
        assert!(!item_drag_crossed_threshold(start, POINT { x: 105, y: 95 }));
        assert!(item_drag_crossed_threshold(start, POINT { x: 106, y: 100 }));
        assert!(item_drag_crossed_threshold(start, POINT { x: 100, y: 94 }));
    }

    #[test]
    fn item_selection_supports_plain_control_and_shift_clicks() {
        let items = selection_items();
        let mut selected = HashSet::new();
        let mut anchor = None;

        assert!(!apply_item_selection(
            &items,
            &mut selected,
            &mut anchor,
            1,
            SelectionModifiers::default(),
        ));
        assert_eq!(selected, HashSet::from([items[1].path.clone()]));

        assert!(!apply_item_selection(
            &items,
            &mut selected,
            &mut anchor,
            3,
            SelectionModifiers {
                control: true,
                shift: false,
            },
        ));
        assert_eq!(
            selected,
            HashSet::from([items[1].path.clone(), items[3].path.clone()])
        );

        assert!(!apply_item_selection(
            &items,
            &mut selected,
            &mut anchor,
            1,
            SelectionModifiers {
                control: false,
                shift: true,
            },
        ));
        assert_eq!(
            selected,
            HashSet::from([
                items[1].path.clone(),
                items[2].path.clone(),
                items[3].path.clone(),
            ])
        );
        assert!(apply_item_selection(
            &items,
            &mut selected,
            &mut anchor,
            2,
            SelectionModifiers::default(),
        ));
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn selected_drag_paths_follow_visible_item_order() {
        let items = selection_items();
        let selected = HashSet::from([items[2].path.clone(), items[0].path.clone()]);
        assert_eq!(
            selected_paths_for_drag(&items, &selected, &items[0].path),
            vec![items[0].path.clone(), items[2].path.clone()]
        );
        assert_eq!(
            selected_paths_for_drag(&items, &selected, &items[1].path),
            vec![items[1].path.clone()]
        );
    }

    #[test]
    fn keyboard_navigation_respects_grid_rows_and_edges() {
        assert_eq!(
            navigation_target(None, 7, 3, NavigationDirection::Right),
            Some(0)
        );
        assert_eq!(
            navigation_target(Some(1), 7, 3, NavigationDirection::Right),
            Some(2)
        );
        assert_eq!(
            navigation_target(Some(2), 7, 3, NavigationDirection::Down),
            Some(5)
        );
        assert_eq!(
            navigation_target(Some(5), 7, 3, NavigationDirection::Down),
            Some(5)
        );
        assert_eq!(
            navigation_target(Some(2), 7, 3, NavigationDirection::Up),
            Some(2)
        );
        assert_eq!(
            navigation_target(Some(5), 7, 3, NavigationDirection::Up),
            Some(2)
        );
        assert_eq!(
            navigation_target(Some(0), 7, 3, NavigationDirection::Left),
            Some(0)
        );
        assert_eq!(
            navigation_target(Some(0), 0, 3, NavigationDirection::Down),
            None
        );
    }

    #[test]
    fn keyboard_navigation_scrolls_only_to_reveal_the_focus() {
        let metrics = GridMetrics {
            columns: 3,
            visible_rows: 2,
            total_rows: 5,
            max_scroll_row: 3,
            ..GridMetrics::default()
        };
        assert_eq!(scroll_row_for_item(0, 5, metrics), 0);
        assert_eq!(scroll_row_for_item(0, 6, metrics), 1);
        assert_eq!(scroll_row_for_item(3, 2, metrics), 0);
        assert_eq!(scroll_row_for_item(2, 14, metrics), 3);
    }

    #[test]
    fn ordered_selection_falls_back_to_the_focused_item() {
        let items = selection_items();
        let selected = HashSet::from([items[3].path.clone(), items[1].path.clone()]);
        assert_eq!(
            selected_paths_in_item_order(&items, &selected, Some(&items[0].path)),
            vec![items[1].path.clone(), items[3].path.clone()]
        );
        assert_eq!(
            selected_paths_in_item_order(&items, &HashSet::new(), Some(&items[2].path)),
            vec![items[2].path.clone()]
        );
        assert!(
            selected_paths_in_item_order(
                &items,
                &HashSet::new(),
                Some(Path::new(r"C:\selection-test\missing.txt")),
            )
            .is_empty()
        );
    }

    #[test]
    fn windows_item_name_validation_rejects_unsafe_names() {
        for name in [
            "",
            ".",
            "..",
            "trailing ",
            "trailing.",
            "bad:name.txt",
            "bad?.txt",
            "CON",
            "con.txt",
            "COM1.log",
            "LPT9",
        ] {
            assert!(
                validate_windows_item_name(name).is_err(),
                "{name:?} should be rejected"
            );
        }
        for name in ["项目计划.docx", "COM10.log", "LPT0", "ordinary name"] {
            assert!(
                validate_windows_item_name(name).is_ok(),
                "{name:?} should be accepted"
            );
        }
    }

    #[test]
    fn new_folder_suggestion_skips_existing_default_names() {
        let unique = UNIX_EPOCH
            .elapsed()
            .expect("system time should follow the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "creel-host-new-folder-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("temporary test directory should be created");
        assert_eq!(available_new_folder_name(&directory).unwrap(), "新建文件夹");
        fs::create_dir(directory.join("新建文件夹"))
            .expect("first folder fixture should be created");
        fs::create_dir(directory.join("新建文件夹 (2)"))
            .expect("second folder fixture should be created");
        assert_eq!(
            available_new_folder_name(&directory).unwrap(),
            "新建文件夹 (3)"
        );
        fs::remove_dir_all(&directory).expect("temporary test directory should be removed");
    }

    #[test]
    fn geometry_history_keeps_the_full_expanded_height() {
        let snapshot = HostFenceSnapshot {
            id: "geometry-history".into(),
            title: "Geometry".into(),
            directory: PathBuf::from(r"C:\geometry-history"),
            x: -240.0,
            y: 80.0,
            width: 420.0,
            height: 360.0,
            color: "sky".into(),
            content_color: "paper".into(),
            collapsed: true,
            locked: false,
            display_anchor: None,
            placement: None,
        };
        assert_eq!(
            FenceGeometry::from_snapshot(&snapshot),
            FenceGeometry {
                x: -240.0,
                y: 80.0,
                width: 420.0,
                height: 360.0,
            }
        );
    }

    #[test]
    fn hidden_title_bar_starts_the_window_at_the_original_content_boundary() {
        let snapshot = HostFenceSnapshot {
            id: "hidden-title-geometry".into(),
            title: "Hidden title".into(),
            directory: PathBuf::from(r"C:\hidden-title-geometry"),
            x: 278.0,
            y: 54.0,
            width: 330.0,
            height: 285.0,
            color: "coral".into(),
            content_color: "paper".into(),
            collapsed: false,
            locked: false,
            display_anchor: None,
            placement: None,
        };
        assert_eq!(window_y(snapshot.y, true), 54);
        assert_eq!(visible_height(&snapshot, true), 285);
        assert_eq!(content_top(true), HEADER_HEIGHT + CONTENT_TOP_PADDING);

        assert_eq!(window_y(snapshot.y, false), 86);
        assert_eq!(visible_height(&snapshot, false), 253);
        assert_eq!(
            content_top(false),
            COMPACT_HEADER_HEIGHT + CONTENT_TOP_PADDING
        );
    }

    #[test]
    fn background_opacity_keeps_a_mouse_hittable_one_alpha_minimum() {
        assert_eq!(panel_background_alpha(0.0), 1);
        assert_eq!(panel_background_alpha(0.5), 128);
        assert_eq!(panel_background_alpha(1.0), 255);
        assert_eq!(panel_background_alpha(-4.0), 1);
        assert_eq!(panel_background_alpha(4.0), 255);
    }

    #[test]
    fn ghost_mode_fades_the_complete_surface_only_while_pointer_is_outside() {
        assert_eq!(ghost_surface_alpha(false, false, 0.0), 255);
        assert_eq!(ghost_surface_alpha(true, true, 0.0), 255);
        assert_eq!(ghost_surface_alpha(true, false, 0.2), 51);
        assert_eq!(ghost_surface_alpha(true, false, 0.0), 0);
        assert_eq!(ghost_surface_alpha(true, false, 1.0), 255);
        assert_eq!(ghost_surface_alpha(true, false, -3.0), 0);
        assert_eq!(ghost_surface_alpha(true, false, 4.0), 255);
        assert_eq!(ghost_surface_alpha(true, false, f64::NAN), 51);
    }

    #[test]
    fn only_a_fully_transparent_automatic_ghost_needs_cursor_polling() {
        let mut preferences = HostPreferencesSnapshot {
            title_opacity: 0.9,
            content_opacity: 0.9,
            show_fence_border: true,
            fence_border_opacity: 0.4,
            icon_size: 46,
            ghost_mode: true,
            ghost_mode_trigger: GhostModeTrigger::Automatic,
            ghost_opacity: 0.0,
            ghost_hotkey: "Ctrl+Alt+G".into(),
            show_hidden_files: false,
            show_fence_titles: true,
        };
        assert!(needs_transparent_ghost_hover_poll(&preferences));

        preferences.ghost_opacity = 0.01;
        assert!(!needs_transparent_ghost_hover_poll(&preferences));
        preferences.ghost_opacity = 0.0;
        preferences.ghost_mode_trigger = GhostModeTrigger::Hotkey;
        assert!(!needs_transparent_ghost_hover_poll(&preferences));
        preferences.ghost_mode_trigger = GhostModeTrigger::Automatic;
        preferences.ghost_mode = false;
        assert!(!needs_transparent_ghost_hover_poll(&preferences));
    }

    #[test]
    fn title_and_content_regions_have_independent_opacity() {
        let content = rgb(245, 239, 229);
        let mut pixels = vec![
            99, 119, 226, 0, // title background in BGRA order
            255, 255, 255, 0, // title text
            229, 239, 245, 0, // content background
            52, 57, 61, 0, // content focus or empty-state text
        ];
        apply_surface_alpha(&mut pixels, content, 64, 128, 255, 2, 1, false);
        assert_eq!(
            [pixels[3], pixels[7], pixels[11], pixels[15]],
            [64, 64, 128, 128]
        );
        assert_eq!(&pixels[4..8], &[64, 64, 64, 64]);
        assert_eq!(&pixels[8..12], &[115, 120, 123, 128]);
    }

    #[test]
    fn frosted_material_adds_subtle_grain_only_to_matching_content_pixels() {
        let content = rgb(229, 238, 241);
        let mut pixels = vec![
            241, 238, 229, 0, 241, 238, 229, 0, 52, 57, 61, 0, 241, 238, 229, 0,
        ];
        apply_surface_alpha(&mut pixels, content, 255, 255, 255, 4, 0, true);
        assert_ne!(&pixels[0..3], &[241, 238, 229]);
        assert_eq!(&pixels[8..11], &[52, 57, 61]);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
    }

    #[test]
    fn ghost_alpha_keeps_transparent_background_hittable() {
        let content = rgb(245, 239, 229);
        let mut pixels = vec![
            99, 119, 226, 0, // header background in BGRA order
            229, 239, 245, 0, // content background
        ];
        apply_surface_alpha(&mut pixels, content, 1, 255, 51, 1, 1, false);
        assert_eq!(pixels[3], 1);
        assert_eq!(pixels[7], 51);
    }

    #[test]
    fn zero_percent_ghost_alpha_makes_the_complete_surface_transparent() {
        let content = rgb(245, 239, 229);
        let mut pixels = vec![
            99, 119, 226, 0, // header background in BGRA order
            229, 239, 245, 0, // content background
        ];
        apply_surface_alpha(&mut pixels, content, 255, 255, 0, 1, 1, false);
        assert_eq!(pixels, vec![0; 8]);
    }

    #[test]
    fn selected_item_uses_translucent_blue_instead_of_an_opaque_pale_background() {
        let mut pixels = vec![0; 3 * 3 * 4];
        let overlay = selection_overlay(&RECT {
            left: 0,
            top: 0,
            right: 3,
            bottom: 3,
        });
        composite_rectangle_overlay(&mut pixels, 3, 3, &overlay, 255);
        let corner = &pixels[0..4];
        let center = &pixels[(4 * 4)..(4 * 5)];
        assert_eq!(corner[3], 132);
        assert_eq!(center[3], 42);
        assert!(center[..3].iter().all(|channel| *channel < 42));
        assert_ne!(&center[..3], &[42, 42, 42]);
    }

    #[test]
    fn border_visibility_and_opacity_are_independent_from_panel_regions() {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 3,
            bottom: 3,
        };
        assert!(fence_border_overlay(&bounds, rgb(12, 34, 56), false, 1.0).is_none());

        let border = fence_border_overlay(&bounds, rgb(12, 34, 56), true, 0.4).unwrap();
        assert_eq!(border.fill_alpha, 0);
        assert_eq!(border.border_alpha, 102);

        let mut pixels = vec![0; 3 * 3 * 4];
        composite_rectangle_overlay(&mut pixels, 3, 3, &border, 255);
        assert_eq!(pixels[3], 102);
        assert_eq!(pixels[(4 * 4) + 3], 0);
    }

    #[test]
    fn ghost_surface_alpha_scales_selection_and_border_overlays() {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 3,
            bottom: 3,
        };
        let mut selection_pixels = vec![0; 3 * 3 * 4];
        composite_rectangle_overlay(&mut selection_pixels, 3, 3, &selection_overlay(&bounds), 51);
        assert_eq!(selection_pixels[3], 26);
        assert_eq!(selection_pixels[(4 * 4) + 3], 8);

        let mut border_pixels = vec![0; 3 * 3 * 4];
        let border = fence_border_overlay(&bounds, rgb(12, 34, 56), true, 1.0).unwrap();
        composite_rectangle_overlay(&mut border_pixels, 3, 3, &border, 51);
        assert_eq!(border_pixels[3], 51);
        assert_eq!(border_pixels[(4 * 4) + 3], 0);
    }

    #[test]
    fn native_color_parser_accepts_arbitrary_rgb_hex() {
        assert_eq!(color_for_name("#12aBef"), rgb(0x12, 0xab, 0xef));
        assert_eq!(content_color_for_name("#0F8342"), rgb(0x0f, 0x83, 0x42));
        assert!(parse_hex_color("#12345").is_none());
        assert!(parse_hex_color("#12XZ89").is_none());
    }

    #[test]
    fn reconstructed_content_alpha_removes_the_rendering_background() {
        let mut transparent_panel = [1, 1, 1, 1];
        composite_reconstructed_pixel(
            &mut transparent_panel,
            &[128, 128, 128, 0],
            &[255, 255, 255, 0],
            255,
        );
        assert_eq!(transparent_panel, [128, 128, 128, 128]);

        // A half-transparent red icon edge rendered over black and white must
        // reconstruct to its premultiplied source color, without retaining the
        // pale panel used by the main surface renderer.
        let mut icon_edge = [1, 1, 1, 1];
        composite_reconstructed_pixel(&mut icon_edge, &[0, 32, 128, 0], &[127, 159, 255, 0], 255);
        assert_eq!(icon_edge, [0, 32, 128, 128]);

        let mut untouched = [1, 1, 1, 1];
        composite_reconstructed_pixel(&mut untouched, &[0, 0, 0, 0], &[255, 255, 255, 0], 255);
        assert_eq!(untouched, [1, 1, 1, 1]);

        let mut ghosted = [1, 1, 1, 1];
        composite_reconstructed_pixel(&mut ghosted, &[255, 255, 255, 0], &[255, 255, 255, 0], 51);
        assert_eq!(ghosted, [51, 51, 51, 51]);
    }

    #[test]
    fn folder_listing_obeys_the_hidden_file_preference() {
        let unique = UNIX_EPOCH
            .elapsed()
            .expect("system time should follow the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "creel-host-hidden-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("temporary test directory should be created");
        fs::write(directory.join("visible.txt"), b"visible")
            .expect("visible fixture should be written");
        fs::write(directory.join(".hidden.txt"), b"hidden")
            .expect("hidden fixture should be written");

        let visible = list_folder(&directory, false);
        assert_eq!(
            visible
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["visible.txt"]
        );
        assert_eq!(list_folder(&directory, true).len(), 2);

        fs::remove_dir_all(&directory).expect("temporary test directory should be removed");
    }

    #[test]
    fn marquee_rectangle_normalizes_reverse_drag_and_intersection() {
        let marquee = selection_rectangle(POINT { x: 100, y: 80 }, POINT { x: 20, y: 40 });
        assert_eq!(
            marquee,
            RECT {
                left: 20,
                top: 40,
                right: 101,
                bottom: 81,
            }
        );
        assert!(rects_intersect(
            &marquee,
            &RECT {
                left: 0,
                top: 0,
                right: 30,
                bottom: 50,
            }
        ));
        assert!(!rects_intersect(
            &marquee,
            &RECT {
                left: 101,
                top: 81,
                right: 120,
                bottom: 100,
            }
        ));
    }

    #[test]
    fn thumbnail_extensions_are_case_insensitive() {
        assert!(thumbnail_candidate(Path::new("poster.JPEG")));
        assert!(thumbnail_candidate(Path::new("clip.MP4")));
        assert!(thumbnail_candidate(Path::new("document.PDF")));
        assert!(!thumbnail_candidate(Path::new("notes.txt")));
    }

    #[test]
    fn file_drop_data_object_round_trips_unicode_paths() {
        let _apartment = ComApartment::initialize().unwrap();
        let expected = vec![
            PathBuf::from(r"C:\Users\Creel\Desktop\项目计划.docx"),
            PathBuf::from(r"D:\素材\图标.png"),
        ];
        let data_object: IDataObject = FileDataObject {
            paths: expected.clone(),
        }
        .into();
        let data_object = Some(data_object);
        let actual = dropped_paths(Ref::from(&data_object)).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn file_data_object_rejects_non_file_formats() {
        let format = FORMATETC {
            cfFormat: 1,
            ..file_drop_format()
        };
        assert!(!accepts_file_drop_format(&format));
    }

    #[test]
    fn shell_bitmap_alpha_normalization_clears_garbage_and_premultiplies_straight_edges() {
        let mut straight = vec![
            255, 64, 32, 0, // 完全透明像素中的垃圾 RGB
            200, 100, 50, 128, // 直通 Alpha，通道值可以大于 Alpha
            30, 20, 10, 255,
        ];
        assert!(normalize_shell_bgra(&mut straight));
        assert_eq!(&straight[0..4], &[0, 0, 0, 0]);
        assert_eq!(&straight[4..8], &[100, 50, 25, 128]);
        assert_eq!(&straight[8..12], &[30, 20, 10, 255]);

        let mut premultiplied = vec![100, 50, 25, 128, 9, 8, 7, 0];
        assert!(normalize_shell_bgra(&mut premultiplied));
        assert_eq!(&premultiplied[0..4], &[100, 50, 25, 128]);
        assert_eq!(&premultiplied[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn windows_shell_exposes_a_real_bitmap_for_the_host_binary() {
        let _apartment = ComApartment::initialize().unwrap();
        let executable = std::env::current_exe().unwrap();
        let bitmap = extract_shell_visual(&executable, 48)
            .expect("Windows Shell should expose the test executable icon");
        assert_eq!(bitmap.kind, ShellVisualKind::Icon);
        assert!(bitmap.width > 0);
        assert!(bitmap.height > 0);
        if bitmap.uses_alpha {
            let pixels =
                read_bitmap_pixels(bitmap.handle, bitmap.width, bitmap.height.unsigned_abs())
                    .expect("normalized icon pixels should remain readable");
            assert!(pixels.as_chunks::<4>().0.iter().all(|pixel| {
                pixel[3] != 0 || (pixel[0] == 0 && pixel[1] == 0 && pixel[2] == 0)
            }));
        }
    }

    #[test]
    fn windows_shell_exposes_the_project_image_thumbnail() {
        let _apartment = ComApartment::initialize().unwrap();
        let image = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .expect("workspace root should exist")
            .join("Creel .png");
        let bitmap = extract_shell_visual(&image, 64)
            .expect("Windows Shell should expose the project PNG thumbnail");
        assert_eq!(bitmap.kind, ShellVisualKind::Thumbnail);
        assert!(bitmap.width > 0);
        assert!(bitmap.height > 0);
    }
}
