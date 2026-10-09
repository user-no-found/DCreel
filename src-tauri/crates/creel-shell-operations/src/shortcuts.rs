use crate::shell_path;
use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ},
            Ole::{OleInitialize, OleUninitialize},
        },
        UI::Shell::{
            FOF_NO_CONNECTED_ELEMENTS, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            FOFX_ADDUNDORECORD, FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation,
            IFileOperation, IShellItem, IShellLinkW, SHCreateItemFromParsingName, ShellLink,
        },
    },
    core::{Interface, PCWSTR},
};

struct Apartment;
impl Apartment {
    fn initialize() -> Result<Self, String> {
        unsafe { OleInitialize(None) }
            .map_err(|error| format!("无法初始化 Windows Shell：{error}"))?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

/// Requires a COM apartment on the calling thread. PIDL-only links are not
/// classified as broken: ShellExecute can open them without a filesystem target.
pub fn missing_shortcut_target(path: &Path) -> Result<Option<PathBuf>, String> {
    if !is_shortcut(path) {
        return Ok(None);
    }
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("无法读取快捷方式：{error}"))?;
        let persisted: IPersistFile = link.cast().map_err(|error| error.to_string())?;
        // Keep the filesystem spelling for loading the .lnk, as the host did
        // before. Only Shell item parsing/open/delete receives a converted copy.
        let shortcut: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        persisted
            .Load(PCWSTR(shortcut.as_ptr()), STGM_READ)
            .map_err(|error| format!("快捷方式文件已损坏或无法读取：{error}"))?;
        let mut target = vec![0_u16; 32_768];
        link.GetPath(&mut target, std::ptr::null_mut(), 0)
            .map_err(|error| format!("无法读取快捷方式目标：{error}"))?;
        let length = target
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(target.len());
        if length == 0 {
            return Ok(None);
        }
        let target = PathBuf::from(OsString::from_wide(&target[..length]));
        Ok((!target.exists()).then_some(target))
    }
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
}

/// Recheck the shortcut on this worker's apartment and recycle the link itself.
/// There is deliberately no permanent-delete fallback.
pub fn recycle_broken_shortcut(path: &Path) -> Result<(), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("无法读取快捷方式：{error}"))?;
    if !is_shortcut(path) || !metadata.file_type().is_file() {
        return Err("只能删除此通知对应的失效快捷方式文件".into());
    }
    let _apartment = Apartment::initialize()?;
    if missing_shortcut_target(path)?.is_none() {
        return Err("快捷方式目标已恢复或无法确认失效，未删除".into());
    }
    unsafe {
        let wide = shell_path(path);
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)
            .map_err(|error| format!("无法定位快捷方式：{error}"))?;
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| format!("无法创建回收站操作：{error}"))?;
        operation
            .SetOperationFlags(
                FOFX_RECYCLEONDELETE
                    | FOFX_ADDUNDORECORD
                    | FOFX_EARLYFAILURE
                    | FOF_SILENT
                    | FOF_NOCONFIRMATION
                    | FOF_NOERRORUI
                    | FOF_NO_CONNECTED_ELEMENTS,
            )
            .map_err(|error| format!("无法设置回收站操作：{error}"))?;
        operation
            .DeleteItem(&item, None)
            .map_err(|error| format!("无法安排删除快捷方式：{error}"))?;
        operation
            .PerformOperations()
            .map_err(|error| format!("无法将快捷方式移入回收站：{error}"))?;
        let aborted = operation
            .GetAnyOperationsAborted()
            .map_err(|error| error.to_string())?;
        if aborted.as_bool() || path.exists() {
            return Err("快捷方式未移入回收站，请重试".into());
        }
    }
    Ok(())
}
