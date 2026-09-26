//! 启动时的映射找回：程序关闭期间被改名或同卷移动的目录，凭持久化的卷身份
//! 与完整文件 ID 重新定位。这里只做一次性的启动核对，不监听运行期改名，
//! 因为运行中的目录随时可能被其他进程占用。

use crate::directory_identity::DirectoryIdentity;
use crate::directory_identity::{self, DirectoryProbe, LookupOutcome, ProbeOutcome};
use crate::models::FenceConfig;
use crate::store::AppStore;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// 目录还在保存的位置上，身份吻合。
    Intact,
    /// 旧配置没有身份，本次采集并补齐。
    IdentityRecorded,
    /// 按身份找回了目录，配置已更新。
    Relocated { previous: PathBuf, current: PathBuf },
    /// 保存的路径已被另一个目录占用，且找不回原来的目录。
    Replaced,
    /// 目录不存在，并且卷上已经找不到这个文件 ID。
    Missing,
    /// 卷未挂载、文件系统不支持或权限不足，无法尝试找回。
    Unsupported,
    /// 目录还在但身份读不出来，保守保留现有配置。
    Unverifiable,
    /// 找回的位置会和其他盒子重复，拒绝改动。
    Conflicted,
}

impl RecoveryOutcome {
    pub fn describes(&self) -> &'static str {
        match self {
            Self::Intact => "intact",
            Self::IdentityRecorded => "identity recorded",
            Self::Relocated { .. } => "relocated",
            Self::Replaced => "replaced by another directory",
            Self::Missing => "missing",
            Self::Unsupported => "unsupported",
            Self::Unverifiable => "unverifiable",
            Self::Conflicted => "conflicts with another box",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceRecovery {
    pub id: String,
    pub title: String,
    pub outcome: RecoveryOutcome,
}

/// 待应用的找回结果。冲突判定通过后才会写回配置。
struct Relocation {
    previous: PathBuf,
    directory: PathBuf,
    identity: DirectoryIdentity,
}

pub fn recover_at_startup(store: &AppStore) {
    let reports = recover_store(store, directory_identity::system_probe());
    log_reports(&reports);
}

pub fn recover_store(store: &AppStore, probe: &dyn DirectoryProbe) -> Vec<FenceRecovery> {
    let mut state = match store.lock() {
        Ok(state) => state,
        Err(error) => {
            log::error!(target: "recovery", "application state is unavailable: {error}");
            return Vec::new();
        }
    };
    let mut fences = state.fences.clone();
    let reports = recover_fences(&mut fences, probe);
    let changed = reports.iter().any(|report| {
        matches!(
            report.outcome,
            RecoveryOutcome::IdentityRecorded | RecoveryOutcome::Relocated { .. }
        )
    });
    if !changed {
        return reports;
    }
    let previous = std::mem::replace(&mut state.fences, fences);
    if let Err(error) = store.save(&state) {
        // 落盘失败就整体回滚：内存与磁盘都保持启动时读到的配置。
        state.fences = previous;
        log::error!(
            target: "recovery",
            "recovered mappings could not be persisted and were rolled back: {error}"
        );
        return Vec::new();
    }
    reports
}

fn log_reports(reports: &[FenceRecovery]) {
    for report in reports {
        match &report.outcome {
            RecoveryOutcome::Relocated { previous, current } => log::info!(
                target: "recovery",
                "box \"{}\" moved from {} to {}",
                report.title,
                previous.display(),
                current.display(),
            ),
            RecoveryOutcome::Intact | RecoveryOutcome::IdentityRecorded => {}
            outcome => log::warn!(
                target: "recovery",
                "box \"{}\" ({}) keeps its stored mapping: {}",
                report.title,
                report.id,
                outcome.describes(),
            ),
        }
    }
}

/// 纯逻辑：就地修正盒子列表的目录与身份，并给出每只盒子的处置结论。
pub fn recover_fences(
    fences: &mut [FenceConfig],
    probe: &dyn DirectoryProbe,
) -> Vec<FenceRecovery> {
    let mut outcomes = Vec::with_capacity(fences.len());
    let mut relocations: Vec<Option<Relocation>> = Vec::with_capacity(fences.len());
    for fence in fences.iter_mut() {
        let (outcome, relocation) = examine(fence, probe);
        outcomes.push(outcome);
        relocations.push(relocation);
    }
    reject_conflicts(fences, &mut relocations, &mut outcomes);
    for (index, relocation) in relocations.into_iter().enumerate() {
        let Some(relocation) = relocation else {
            continue;
        };
        fences[index].directory = relocation.directory;
        fences[index].directory_identity = Some(relocation.identity);
        outcomes[index] = RecoveryOutcome::Relocated {
            previous: relocation.previous,
            current: fences[index].directory.clone(),
        };
    }
    fences
        .iter()
        .zip(outcomes)
        .map(|(fence, outcome)| FenceRecovery {
            id: fence.id.clone(),
            title: fence.title.clone(),
            outcome,
        })
        .collect()
}

fn examine(
    fence: &mut FenceConfig,
    probe: &dyn DirectoryProbe,
) -> (RecoveryOutcome, Option<Relocation>) {
    // 身份残缺的旧配置按“没有身份”处理，绝不能拿它去做找回。
    let saved = fence
        .directory_identity
        .clone()
        .filter(DirectoryIdentity::is_usable);
    match (probe.probe(&fence.directory), saved) {
        (ProbeOutcome::Present(current), None) => {
            fence.directory_identity = Some(current);
            (RecoveryOutcome::IdentityRecorded, None)
        }
        (ProbeOutcome::Present(current), Some(saved)) => {
            if current.same_directory(&saved) {
                return (RecoveryOutcome::Intact, None);
            }
            // 同名路径已经是别的目录：只有原目录的身份能证明找回结果。
            relocate(fence, &saved, RecoveryOutcome::Replaced, probe)
        }
        (ProbeOutcome::Absent, Some(saved)) => {
            relocate(fence, &saved, RecoveryOutcome::Missing, probe)
        }
        // 旧配置且目录已经没了：没有身份可以依据，留给用户手动修复。
        (ProbeOutcome::Absent, None) => (RecoveryOutcome::Missing, None),
        (ProbeOutcome::Indeterminate, _) => (RecoveryOutcome::Unverifiable, None),
    }
}

fn relocate(
    fence: &FenceConfig,
    saved: &DirectoryIdentity,
    fallback: RecoveryOutcome,
    probe: &dyn DirectoryProbe,
) -> (RecoveryOutcome, Option<Relocation>) {
    let (directory, identity) = match probe.lookup(saved) {
        LookupOutcome::Found {
            directory,
            identity,
        } => (directory, identity),
        LookupOutcome::NotFound => return (fallback, None),
        LookupOutcome::Unsupported => return (RecoveryOutcome::Unsupported, None),
    };
    if !identity.same_directory(saved) {
        return (fallback, None);
    }
    // 找回的路径要重新打开核对：目录属性、卷身份和完整文件 ID 全部吻合才改。
    match probe.probe(&directory) {
        ProbeOutcome::Present(verified) if verified.same_directory(saved) => (
            fallback,
            Some(Relocation {
                previous: fence.directory.clone(),
                directory,
                identity: verified,
            }),
        ),
        _ => (fallback, None),
    }
}

/// 两只盒子不能指向同一个目录：找回结果一旦和别处重合就整体拒绝该次找回。
fn reject_conflicts(
    fences: &[FenceConfig],
    relocations: &mut [Option<Relocation>],
    outcomes: &mut [RecoveryOutcome],
) {
    loop {
        let mut by_identity: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_path: HashMap<String, Vec<usize>> = HashMap::new();
        for index in 0..fences.len() {
            let (directory, identity) = match relocations[index].as_ref() {
                Some(relocation) => (relocation.directory.as_path(), Some(&relocation.identity)),
                None => (
                    fences[index].directory.as_path(),
                    fences[index].directory_identity.as_ref(),
                ),
            };
            if let Some(identity) = identity.filter(|identity| identity.is_usable()) {
                by_identity
                    .entry(identity_key(identity))
                    .or_default()
                    .push(index);
            }
            by_path.entry(path_key(directory)).or_default().push(index);
        }
        let mut rejected = false;
        for group in by_identity
            .values()
            .chain(by_path.values())
            .filter(|group| group.len() > 1)
        {
            for index in group.clone() {
                if relocations[index].take().is_some() {
                    outcomes[index] = RecoveryOutcome::Conflicted;
                    rejected = true;
                }
            }
        }
        if !rejected {
            return;
        }
    }
}

pub(crate) fn identity_key(identity: &DirectoryIdentity) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        identity.volume_guid_path.to_lowercase(),
        identity.volume_serial,
        identity.file_id_low,
        identity.file_id_high,
        identity.creation_time
    )
}

fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy().to_lowercase();
    let text = match text.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix("unc\\") {
            Some(share) => format!(r"\\{share}"),
            None => rest.to_string(),
        },
        None => text,
    };
    let trimmed = text.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        text
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod persistence_tests;
#[cfg(test)]
mod tests;
