//! Windows 目录身份：从打开的目录句柄读取卷身份 + 完整 128 位文件 ID，
//! 程序关闭期间目录被改名或同卷移动后，启动时按这个身份找回。
//!
//! Win32 调用全部收敛在本模块：`handles` 负责句柄与共享模式，`native_paths`
//! 负责卷与路径转换，`file_ids` 负责 ID 读取与按 ID 打开。这里只保留判定顺序。

mod file_ids;
mod handles;
mod native_paths;

use crate::directory_identity::DirectoryIdentity;
use crate::directory_identity::{DirectoryProbe, LookupOutcome, ProbeOutcome};
use std::path::Path;
use windows::core::Error;

pub struct WindowsDirectoryProbe;

impl DirectoryProbe for WindowsDirectoryProbe {
    fn probe(&self, path: &Path) -> ProbeOutcome {
        let handle = match handles::open_directory(path) {
            Ok(handle) => handle,
            Err(error) => return classify_open_failure(&error),
        };
        match file_ids::read_identity(&handle, path) {
            file_ids::ReadOutcome::Identity(identity) => ProbeOutcome::Present(identity),
            file_ids::ReadOutcome::NotDirectory => ProbeOutcome::Absent,
            file_ids::ReadOutcome::Indeterminate => ProbeOutcome::Indeterminate,
        }
    }

    fn lookup(&self, saved: &DirectoryIdentity) -> LookupOutcome {
        if !saved.is_usable() {
            return LookupOutcome::NotFound;
        }
        let volume = match handles::open_volume(&saved.volume_guid_path) {
            Ok(volume) => volume,
            Err(error) => {
                log::warn!(
                    target: "recovery",
                    "volume {} is not reachable right now: {error}",
                    saved.volume_guid_path
                );
                return LookupOutcome::Unsupported;
            }
        };
        // 首选类型来自保存时的文件系统；另一种只作兜底，命中后仍要核对身份。
        let mut missing = false;
        for kind in [saved.id_kind, file_ids::other_kind(saved.id_kind)] {
            let handle = match file_ids::open_by_id(&volume, saved, kind) {
                Ok(handle) => handle,
                Err(error) => {
                    if handles::is_unresolvable(&error) {
                        missing = true;
                    } else {
                        log::warn!(
                            target: "recovery",
                            "could not open {} by {kind:?}: {error}",
                            saved.volume_guid_path
                        );
                    }
                    continue;
                }
            };
            // 打开成功但核对不过：这个 ID 已经属于别的东西，或当前校验不了。
            let Some(found) = file_ids::verified_identity(&handle, saved) else {
                missing = true;
                continue;
            };
            let Some(directory) = native_paths::final_path(handle.raw()) else {
                missing = true;
                continue;
            };
            return LookupOutcome::Found {
                directory,
                identity: found,
            };
        }
        if missing {
            LookupOutcome::NotFound
        } else {
            LookupOutcome::Unsupported
        }
    }
}

/// 打不开目录时，只有确认路径已经不在了才报“缺失”，其余一律不下结论。
fn classify_open_failure(error: &Error) -> ProbeOutcome {
    if handles::is_unresolvable(error) {
        ProbeOutcome::Absent
    } else {
        ProbeOutcome::Indeterminate
    }
}

#[cfg(test)]
mod tests;
