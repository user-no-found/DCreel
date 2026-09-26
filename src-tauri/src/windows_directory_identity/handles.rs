//! 目录与卷句柄：以只读属性 + FULL sharing 打开，并在离开作用域时立刻归还，
//! 免得这个功能自己把目录占住导致用户无法改名。

use super::native_paths::wide_path;
use std::ffi::OsStr;
use std::path::Path;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BAD_PATHNAME, ERROR_CANT_ACCESS_FILE, ERROR_FILE_NOT_FOUND,
    ERROR_INVALID_NAME, ERROR_INVALID_PARAMETER, ERROR_MOUNT_POINT_NOT_RESOLVED,
    ERROR_PATH_NOT_FOUND, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAGS_AND_ATTRIBUTES, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::core::{Error, HRESULT, PCWSTR};

/// 只请求读属性，配合 READ|WRITE|DELETE 全共享，保证不阻止改名和删除。
pub(crate) const DESIRED_ACCESS: u32 = FILE_READ_ATTRIBUTES.0;
pub(crate) const SHARE_MODE: FILE_SHARE_MODE =
    FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);

pub(crate) struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// 接管一个刚刚成功打开、尚未被任何地方持有的句柄。
    pub(crate) fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 打开目录句柄。`FILE_FLAG_BACKUP_SEMANTICS` 是打开文件夹的前提条件。
pub(crate) fn open_directory(path: &Path) -> Result<OwnedHandle, Error> {
    let buffer = wide_path(path.as_os_str())
        .ok_or_else(|| Error::from_hresult(HRESULT::from_win32(ERROR_INVALID_NAME.0)))?;
    unsafe {
        CreateFileW(
            PCWSTR(buffer.as_ptr()),
            DESIRED_ACCESS,
            SHARE_MODE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map(OwnedHandle::new)
}

/// 打开卷句柄，只用来给 `OpenFileById` 指明卷——按官方定义它是“该卷上任意
/// 文件的句柄”，卷根本身就是最贴切的选择；访问权限必须为 0。
/// 保存的规范形式是 `\\?\Volume{guid}\`，但 `CreateFileW` 只接受不带结尾
/// 反斜杠的卷路径，所以打开前要去掉。
pub(crate) fn open_volume(volume_guid_path: &str) -> Result<OwnedHandle, Error> {
    let trimmed = volume_guid_path.trim_end_matches('\\');
    let buffer = wide_path(OsStr::new(trimmed))
        .ok_or_else(|| Error::from_hresult(HRESULT::from_win32(ERROR_INVALID_NAME.0)))?;
    unsafe {
        CreateFileW(
            PCWSTR(buffer.as_ptr()),
            0,
            SHARE_MODE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(OwnedHandle::new)
}

/// 表示“这个路径/这个文件 ID 当前解析不到任何东西”的错误码，用来把它和
/// “根本没去解析”（卷不可达、权限不足、功能不支持）区分开。
/// 注意：NTFS 对已经释放的文件 ID 返回的是 `ERROR_INVALID_PARAMETER`，
/// 不是 `ERROR_FILE_NOT_FOUND`。
pub(crate) fn is_unresolvable(error: &Error) -> bool {
    [
        ERROR_FILE_NOT_FOUND,
        ERROR_PATH_NOT_FOUND,
        ERROR_BAD_PATHNAME,
        ERROR_INVALID_NAME,
        ERROR_INVALID_PARAMETER,
        ERROR_CANT_ACCESS_FILE,
        ERROR_MOUNT_POINT_NOT_RESOLVED,
    ]
    .iter()
    .any(|code| error.code() == HRESULT::from_win32(code.0))
}
