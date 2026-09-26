//! 启动恢复与新建映射共用的目录身份：`DirectoryIdentity` 是持久化用的值类型，
//! `DirectoryProbe` 是读取与找回它的接口。Windows 实现见 `windows_directory_identity`，
//! 接口留在这里，恢复逻辑才能在任意平台被完整测试。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectoryIdKind {
    /// 文件系统支持 64 位文件 ID（NTFS 的传统形态）。
    #[default]
    FileId64,
    /// 必须把完整 128 位文件 ID 交给 `OpenFileById`（ReFS 等）。
    ExtendedFileId128,
}

/// 目录在卷内的稳定身份。程序关闭期间目录被改名或同卷移动后，启动时
/// 可以凭它找回原目录，而不是绑到同名新建的目录上。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryIdentity {
    /// `\\?\Volume{guid}\`，比盘符稳定，用于重新打开正确的卷。
    #[serde(default)]
    pub volume_guid_path: String,
    #[serde(default)]
    pub volume_serial: u64,
    /// 完整 128 位文件 ID 的两个半部，按小端拆分，绝不截断保存。
    #[serde(default)]
    pub file_id_low: u64,
    #[serde(default)]
    pub file_id_high: u64,
    #[serde(default)]
    pub id_kind: DirectoryIdKind,
    /// 记录用的文件系统名称，例如 `NTFS`、`ReFS`。
    #[serde(default)]
    pub file_system: String,
    /// 目录创建时间（FILETIME 刻度）。NTFS 会复用 MFT 记录号，仅凭 ID 认目录
    /// 并不保险，创建时间是 ID 之外第二道“还是原来那个目录”的依据。
    /// 0 表示平台没有提供，比较时按“未知”处理而不下结论。
    #[serde(default)]
    pub creation_time: i64,
}

impl DirectoryIdentity {
    /// 全零或没有卷标识的身份都不可信，不能拿去做找回。
    pub fn is_usable(&self) -> bool {
        !self.volume_guid_path.is_empty() && (self.file_id_low != 0 || self.file_id_high != 0)
    }

    /// 卷、完整文件 ID 与创建时间是否指向同一个目录。文件系统名称只作诊断，
    /// 不参与比较。
    pub fn same_directory(&self, other: &Self) -> bool {
        self.volume_serial == other.volume_serial
            && self.file_id_low == other.file_id_low
            && self.file_id_high == other.file_id_high
            && self
                .volume_guid_path
                .eq_ignore_ascii_case(&other.volume_guid_path)
            && (self.creation_time == 0
                || other.creation_time == 0
                || self.creation_time == other.creation_time)
    }
}

/// 读取一个现有路径的目录身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// 路径可以按文件夹打开，并且读到了稳定身份。
    Present(DirectoryIdentity),
    /// 路径不存在，或者已经被别的东西占用，不是当初的文件夹。
    Absent,
    /// 文件夹在，但身份读不出来或无法校验（不支持的文件系统、权限不足等）。
    Indeterminate,
}

/// 按保存的身份重新定位目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupOutcome {
    /// 找回了目录，`identity` 是从最终路径重新读取到的身份。
    Found {
        directory: PathBuf,
        identity: DirectoryIdentity,
    },
    /// 卷上已经没有这个 ID（目录被删除，或跨卷搬移后 ID 已变）。
    NotFound,
    /// 平台或文件系统不支持按 ID 打开，或者卷当前不可访问。
    Unsupported,
}

/// 启动恢复使用的目录身份探针。抽象出来是为了让恢复逻辑可以在任何平台
/// 被完整测试，也保证 Windows API 只出现在平台专属模块里。
pub trait DirectoryProbe: Send + Sync {
    fn probe(&self, path: &Path) -> ProbeOutcome;
    fn lookup(&self, identity: &DirectoryIdentity) -> LookupOutcome;
}

#[cfg(windows)]
pub fn system_probe() -> &'static dyn DirectoryProbe {
    use crate::windows_directory_identity::WindowsDirectoryProbe;
    &WindowsDirectoryProbe
}

/// 新建或改动映射时采集目录身份；读不出来就保持 None，留给启动时再补。
pub fn capture_identity(path: &Path) -> Option<DirectoryIdentity> {
    match system_probe().probe(path) {
        ProbeOutcome::Present(identity) => Some(identity),
        _ => None,
    }
}

/// 非 Windows 平台保持原有行为：不做任何找回，也不伪造身份。
#[cfg(not(windows))]
pub fn system_probe() -> &'static dyn DirectoryProbe {
    &UnsupportedProbe
}

#[cfg(not(windows))]
pub struct UnsupportedProbe;

#[cfg(not(windows))]
impl DirectoryProbe for UnsupportedProbe {
    fn probe(&self, _path: &Path) -> ProbeOutcome {
        ProbeOutcome::Indeterminate
    }

    fn lookup(&self, _identity: &DirectoryIdentity) -> LookupOutcome {
        LookupOutcome::Unsupported
    }
}

#[cfg(test)]
mod tests;
