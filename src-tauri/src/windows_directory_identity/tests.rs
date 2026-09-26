//! `windows_directory_identity` 的测试：纯转换逻辑用固定输入核对，
//! 卷/ID 相关的部分在真实文件系统上做端到端验证。

use super::WindowsDirectoryProbe;
use super::file_ids::{self, other_kind, preferred_kind};
use super::native_paths::strip_device_namespace;
use crate::directory_identity::{DirectoryIdKind, DirectoryIdentity};
use crate::directory_identity::{DirectoryProbe, LookupOutcome, ProbeOutcome};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn text(wide: &[u16]) -> String {
    std::ffi::OsString::from_wide(wide)
        .to_string_lossy()
        .into_owned()
}

fn normalized(value: &str) -> Option<String> {
    strip_device_namespace(&wide(value)).map(|result| text(&result))
}

#[test]
fn device_namespace_prefixes_become_usable_paths() {
    assert_eq!(
        normalized(r"\\?\C:\Users\Creel\项目").as_deref(),
        Some(r"C:\Users\Creel\项目")
    );
    assert_eq!(
        normalized(r"\\?\UNC\nas\share\box").as_deref(),
        Some(r"\\nas\share\box")
    );
    assert_eq!(normalized(r"C:\plain").as_deref(), Some(r"C:\plain"));
    assert_eq!(normalized(r"\\?\Volume{1a2b}\box"), None);
}

#[test]
fn only_ntfs_short_ids_use_the_64_bit_descriptor() {
    assert_eq!(preferred_kind("NTFS", 0), DirectoryIdKind::FileId64);
    assert_eq!(
        preferred_kind("NTFS", 1),
        DirectoryIdKind::ExtendedFileId128
    );
    assert_eq!(
        preferred_kind("ReFS", 0),
        DirectoryIdKind::ExtendedFileId128
    );
    assert_eq!(preferred_kind("", 0), DirectoryIdKind::ExtendedFileId128);
    assert_eq!(
        other_kind(DirectoryIdKind::FileId64),
        DirectoryIdKind::ExtendedFileId128
    );
    assert_eq!(
        other_kind(DirectoryIdKind::ExtendedFileId128),
        DirectoryIdKind::FileId64
    );
}

#[test]
fn stored_file_id_keeps_all_128_bits() {
    let identity = DirectoryIdentity {
        file_id_low: 0x0102_0304_0506_0708,
        file_id_high: 0x090a_0b0c_0d0e_0f10,
        ..DirectoryIdentity::default()
    };
    let bytes = file_ids::file_id_bytes(&identity);
    assert_eq!(&bytes[..8], &0x0102_0304_0506_0708_u64.to_le_bytes());
    assert_eq!(&bytes[8..], &0x090a_0b0c_0d0e_0f10_u64.to_le_bytes());
    assert_eq!(
        file_ids::split_file_id(&bytes),
        Some((identity.file_id_low, identity.file_id_high))
    );
}

/// 真实卷上的验证。临时目录所在卷读不到身份时跳过，而不是伪造成功。
fn identity_of_created(directory: &Path) -> Option<DirectoryIdentity> {
    match WindowsDirectoryProbe.probe(directory) {
        ProbeOutcome::Present(identity) => Some(identity),
        other => {
            eprintln!("skipping: probe of {} gave {other:?}", directory.display());
            None
        }
    }
}

#[test]
fn probe_reads_volume_identity_from_a_live_directory() {
    let Some(identity) = identity_of_created(&std::env::temp_dir()) else {
        return;
    };
    assert!(identity.volume_guid_path.starts_with(r"\\?\Volume{"));
    assert!(identity.volume_guid_path.ends_with('\\'));
    assert!(identity.is_usable());
    assert_eq!(identity.file_system, "NTFS");
}

#[test]
fn renaming_a_directory_while_closed_is_recovered_by_identity() {
    let root = std::env::temp_dir().join(format!("dcreel-identity-{}", uuid::Uuid::new_v4()));
    let original = root.join("原始盒子");
    std::fs::create_dir_all(&original).unwrap();
    let Some(identity) = identity_of_created(&original) else {
        std::fs::remove_dir_all(&root).ok();
        return;
    };

    let renamed = root.join("改名之后的盒子");
    std::fs::rename(&original, &renamed).unwrap();

    assert_eq!(
        WindowsDirectoryProbe.probe(&original),
        ProbeOutcome::Absent,
        "原路径已经不存在"
    );
    match WindowsDirectoryProbe.lookup(&identity) {
        LookupOutcome::Found { directory, .. } => assert_eq!(directory, renamed),
        other => panic!("expected the renamed directory, got {other:?}"),
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_moved_directory_is_found_even_when_the_old_name_is_reused() {
    let root = std::env::temp_dir().join(format!("dcreel-moved-{}", uuid::Uuid::new_v4()));
    let original = root.join("资料").join("盒子");
    std::fs::create_dir_all(&original).unwrap();
    let Some(identity) = identity_of_created(&original) else {
        std::fs::remove_dir_all(&root).ok();
        return;
    };

    let destination = root.join("归档").join("盒子");
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::rename(&original, &destination).unwrap();
    // 用户在同名位置新建了一个空文件夹，冒充原来的目录。
    std::fs::create_dir_all(&original).unwrap();

    match WindowsDirectoryProbe.lookup(&identity) {
        LookupOutcome::Found { directory, .. } => assert_eq!(directory, destination),
        other => panic!("expected the moved directory, got {other:?}"),
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_replacement_directory_is_not_mistaken_for_the_original() {
    let root = std::env::temp_dir().join(format!("dcreel-swapped-{}", uuid::Uuid::new_v4()));
    let original = root.join("盒子");
    std::fs::create_dir_all(&original).unwrap();
    let Some(identity) = identity_of_created(&original) else {
        std::fs::remove_dir_all(&root).ok();
        return;
    };
    std::fs::remove_dir(&original).unwrap();
    // NTFS 会复用 MFT 记录号，创建时间是 ID 之外的第二道依据；这里留出时间差，
    // 避免删除与重建落在同一个时钟刻度上让断言变得不确定。
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::create_dir_all(&original).unwrap();

    let replacement = match WindowsDirectoryProbe.probe(&original) {
        ProbeOutcome::Present(replacement) => replacement,
        other => panic!("expected the new directory, got {other:?}"),
    };
    assert!(
        !replacement.same_directory(&identity),
        "删掉重建的同名目录不能算同一个目录：{identity:?} vs {replacement:?}"
    );
    assert!(
        matches!(
            WindowsDirectoryProbe.lookup(&identity),
            LookupOutcome::NotFound
        ),
        "按旧身份不能落到任何新目录上"
    );
    std::fs::remove_dir_all(root).ok();
}
