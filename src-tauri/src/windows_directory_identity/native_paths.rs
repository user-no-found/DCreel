//! Win32 路径与卷标识转换：UTF-16 缓冲、卷 GUID 路径、最终路径的反规范前缀。

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    GetFinalPathNameByHandleW, GetVolumeInformationByHandleW, GetVolumeNameForVolumeMountPointW,
    GetVolumePathNameW, VOLUME_NAME_DOS,
};
use windows::core::PCWSTR;

const WIDE_BUFFER: usize = 256;
const FILE_SYSTEM_BUFFER: usize = 64;
const PATH_CHUNK: usize = 512;
const MAX_EXTENDED_PATH: usize = 32_767;

/// 转成内核接受的 UTF-16 串。含内嵌 NUL 的路径无法表达，直接放弃。
pub(crate) fn wide_path(value: &OsStr) -> Option<Vec<u16>> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return None;
    }
    wide.push(0);
    Some(wide)
}

/// 交给 `PCWSTR` 之前必须补上终止符，否则内核会读到缓冲区外面。
fn nul_terminate(value: &[u16]) -> Vec<u16> {
    let mut buffer = value.to_vec();
    buffer.push(0);
    buffer
}

/// 截到第一个 NUL；整窗都是 NUL 时视为无效。
fn take_wide(buffer: &[u16]) -> Option<Vec<u16>> {
    let end = buffer.iter().position(|value| *value == 0)?;
    (end > 0).then(|| buffer[..end].to_vec())
}

/// `\\?\Volume{guid}\`：比盘符稳定的卷标识，重新打开卷时用它。
pub(crate) fn volume_guid_path(wide_path: &[u16]) -> Option<String> {
    let mut mount_point = [0_u16; WIDE_BUFFER];
    unsafe { GetVolumePathNameW(PCWSTR(wide_path.as_ptr()), &mut mount_point).ok()? };
    let mount_point = nul_terminate(&take_wide(&mount_point)?);
    let mut guid = [0_u16; WIDE_BUFFER];
    unsafe {
        GetVolumeNameForVolumeMountPointW(PCWSTR(mount_point.as_ptr()), &mut guid).ok()?;
    }
    take_wide(&guid).and_then(|value| String::from_utf16(&value).ok())
}

/// 卷的文件系统名称，例如 `NTFS`、`ReFS`。
pub(crate) fn file_system_name(handle: HANDLE) -> Option<String> {
    let mut name = [0_u16; FILE_SYSTEM_BUFFER];
    let mut serial = 0_u32;
    let mut component_length = 0_u32;
    let mut flags = 0_u32;
    unsafe {
        GetVolumeInformationByHandleW(
            handle,
            None,
            Some(&mut serial),
            Some(&mut component_length),
            Some(&mut flags),
            Some(&mut name),
        )
        .ok()?;
    }
    take_wide(&name).and_then(|value| String::from_utf16(&value).ok())
}

/// 句柄当前所在的 Win32 路径。`GetFinalPathNameByHandleW` 给出的是
/// `\\?\C:\...` 这类内核路径，去掉前缀才是界面和 Explorer 能用的形式。
pub(crate) fn final_path(handle: HANDLE) -> Option<PathBuf> {
    let mut capacity = PATH_CHUNK;
    loop {
        let mut buffer = vec![0_u16; capacity];
        let length = unsafe { GetFinalPathNameByHandleW(handle, &mut buffer, VOLUME_NAME_DOS) };
        if length == 0 {
            return None;
        }
        let needed = length as usize;
        if needed < capacity {
            let stripped = strip_device_namespace(&buffer[..needed])?;
            return Some(PathBuf::from(OsString::from_wide(&stripped)));
        }
        if needed > MAX_EXTENDED_PATH {
            return None;
        }
        capacity = needed + 1;
    }
}

pub(crate) fn strip_device_namespace(wide: &[u16]) -> Option<Vec<u16>> {
    const DEVICE_NS: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const UNC: [u16; 4] = [b'U' as u16, b'N' as u16, b'C' as u16, b'\\' as u16];
    const VOLUME: [u16; 7] = [
        b'V' as u16,
        b'o' as u16,
        b'l' as u16,
        b'u' as u16,
        b'm' as u16,
        b'e' as u16,
        b'{' as u16,
    ];
    let Some(rest) = wide.strip_prefix(&DEVICE_NS) else {
        return Some(wide.to_vec());
    };
    // 卷 Namespace 路径不是一条能长期使用的 Win32 路径，宁可不改也不写回配置。
    if rest.starts_with(&VOLUME) {
        return None;
    }
    if let Some(share) = rest.strip_prefix(&UNC) {
        let mut normalized = vec![b'\\' as u16, b'\\' as u16];
        normalized.extend_from_slice(share);
        return Some(normalized);
    }
    Some(rest.to_vec())
}
