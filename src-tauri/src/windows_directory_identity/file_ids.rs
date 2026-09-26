//! 文件 ID：从目录句柄读取卷序列号、完整 128 位 ID 与创建时间，并按 ID
//! 重新打开目录。64 位与 128 位描述符的选择按文件系统语义区分。

use super::handles::{DESIRED_ACCESS, OwnedHandle, SHARE_MODE};
use super::native_paths;
use crate::directory_identity::{DirectoryIdKind, DirectoryIdentity};
use std::mem::size_of;
use std::path::Path;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    ExtendedFileIdType, FILE_ATTRIBUTE_DIRECTORY, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_ID_128, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS,
    FileBasicInfo, FileIdInfo, FileIdType, GetFileInformationByHandleEx, OpenFileById,
};
use windows::core::Error;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadOutcome {
    /// 是一个文件夹，并且读到了可用的卷身份与文件 ID。
    Identity(DirectoryIdentity),
    /// 路径能打开，但不是文件夹。
    NotDirectory,
    /// 身份、卷标识或属性读不出来，无法下任何结论。
    Indeterminate,
}

/// 读取句柄所在目录的身份。卷标识从路径推导；只有在按 ID 打开、拿不到路径时
/// 才由调用方把已保存的卷标识带回来。
pub(crate) fn read_identity(handle: &OwnedHandle, path: &Path) -> ReadOutcome {
    let Some((is_directory, creation_time)) = directory_time(handle.raw()) else {
        return ReadOutcome::Indeterminate;
    };
    if !is_directory {
        return ReadOutcome::NotDirectory;
    }
    let Some((volume_serial, file_id_low, file_id_high)) = file_id_of(handle.raw()) else {
        return ReadOutcome::Indeterminate;
    };
    let Some(wide) = native_paths::wide_path(path.as_os_str()) else {
        return ReadOutcome::Indeterminate;
    };
    let Some(volume_guid_path) = native_paths::volume_guid_path(&wide) else {
        return ReadOutcome::Indeterminate;
    };
    // 文件系统名称只影响描述符的首选类型，读不到时按 128 位保守处理。
    let file_system = native_paths::file_system_name(handle.raw()).unwrap_or_default();
    let identity = DirectoryIdentity {
        volume_guid_path,
        volume_serial,
        file_id_low,
        file_id_high,
        id_kind: preferred_kind(&file_system, file_id_high),
        creation_time,
        file_system,
    };
    if identity.is_usable() {
        ReadOutcome::Identity(identity)
    } else {
        ReadOutcome::Indeterminate
    }
}

/// 在按 ID 打开的句柄上重新核对：确实是文件夹、卷身份一致、完整文件 ID 一致，
/// 并且创建时间也对得上（两边都读到时）。
pub(crate) fn verified_identity(
    handle: &OwnedHandle,
    saved: &DirectoryIdentity,
) -> Option<DirectoryIdentity> {
    let (is_directory, creation_time) = directory_time(handle.raw())?;
    if !is_directory {
        return None;
    }
    let (volume_serial, file_id_low, file_id_high) = file_id_of(handle.raw())?;
    let found = DirectoryIdentity {
        volume_guid_path: saved.volume_guid_path.clone(),
        volume_serial,
        file_id_low,
        file_id_high,
        id_kind: preferred_kind(&saved.file_system, file_id_high),
        creation_time,
        file_system: saved.file_system.clone(),
    };
    found.same_directory(saved).then_some(found)
}

/// NTFS 的短文件 ID 才能用 64 位描述符；其他文件系统（含 ReFS）必须把完整
/// 128 位交给 `OpenFileById`，不能拿低半截去猜。
pub(crate) fn preferred_kind(file_system: &str, file_id_high: u64) -> DirectoryIdKind {
    if file_id_high == 0 && file_system.eq_ignore_ascii_case("NTFS") {
        DirectoryIdKind::FileId64
    } else {
        DirectoryIdKind::ExtendedFileId128
    }
}

pub(crate) fn other_kind(kind: DirectoryIdKind) -> DirectoryIdKind {
    match kind {
        DirectoryIdKind::FileId64 => DirectoryIdKind::ExtendedFileId128,
        DirectoryIdKind::ExtendedFileId128 => DirectoryIdKind::FileId64,
    }
}

/// 按指定描述符类型打开目录。错误留给调用方分类：ID 已失效，还是卷/权限
/// 根本不支持，不能一律当成“找不到”。
pub(crate) fn open_by_id(
    volume: &OwnedHandle,
    saved: &DirectoryIdentity,
    kind: DirectoryIdKind,
) -> Result<OwnedHandle, Error> {
    let descriptor = FILE_ID_DESCRIPTOR {
        dwSize: size_of::<FILE_ID_DESCRIPTOR>() as u32,
        Type: match kind {
            DirectoryIdKind::FileId64 => FileIdType,
            DirectoryIdKind::ExtendedFileId128 => ExtendedFileIdType,
        },
        Anonymous: match kind {
            DirectoryIdKind::FileId64 => FILE_ID_DESCRIPTOR_0 {
                FileId: saved.file_id_low as i64,
            },
            // ReFS 只认 ExtendedFileIdType 携带的完整 128 位。
            DirectoryIdKind::ExtendedFileId128 => FILE_ID_DESCRIPTOR_0 {
                ExtendedFileId: FILE_ID_128 {
                    Identifier: file_id_bytes(saved),
                },
            },
        },
    };
    unsafe {
        OpenFileById(
            volume.raw(),
            &descriptor,
            DESIRED_ACCESS,
            SHARE_MODE,
            None,
            FILE_FLAG_BACKUP_SEMANTICS,
        )
    }
    .map(OwnedHandle::new)
}

pub(crate) fn file_id_bytes(identity: &DirectoryIdentity) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&identity.file_id_low.to_le_bytes());
    bytes[8..].copy_from_slice(&identity.file_id_high.to_le_bytes());
    bytes
}

pub(crate) fn split_file_id(bytes: &[u8; 16]) -> Option<(u64, u64)> {
    let low: [u8; 8] = bytes[..8].try_into().ok()?;
    let high: [u8; 8] = bytes[8..].try_into().ok()?;
    Some((u64::from_le_bytes(low), u64::from_le_bytes(high)))
}

/// (卷序列号, 文件 ID 低 64 位, 文件 ID 高 64 位)。
fn file_id_of(handle: HANDLE) -> Option<(u64, u64, u64)> {
    let info: FILE_ID_INFO = query(handle, FileIdInfo)?;
    let (low, high) = split_file_id(&info.FileId.Identifier)?;
    Some((info.VolumeSerialNumber, low, high))
}

/// (是否目录, 创建时间)。
fn directory_time(handle: HANDLE) -> Option<(bool, i64)> {
    let info: FILE_BASIC_INFO = query(handle, FileBasicInfo)?;
    Some((
        info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
        info.CreationTime,
    ))
}

fn query<T>(handle: HANDLE, class: FILE_INFO_BY_HANDLE_CLASS) -> Option<T> {
    // 只用于 FILE_ID_INFO / FILE_BASIC_INFO 这类纯值结构体。
    let mut buffer: T = unsafe { std::mem::zeroed() };
    let size = size_of::<T>() as u32;
    unsafe {
        GetFileInformationByHandleEx(handle, class, std::ptr::addr_of_mut!(buffer).cast(), size)
            .ok()
            .map(|()| buffer)
    }
}
