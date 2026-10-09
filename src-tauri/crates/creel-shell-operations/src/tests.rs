use crate::{missing_shortcut_target, recycle_broken_shortcut, shell_path};
use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile},
            Ole::{OleInitialize, OleUninitialize},
        },
        UI::Shell::{IShellLinkW, SHParseDisplayName, ShellLink},
    },
    core::{Interface, PCWSTR},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dcreel-shortcut-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        unsafe { OleInitialize(None) }.unwrap();
        Self(path)
    }
    fn link(&self, target: &Path, name: &str) -> PathBuf {
        let path = self.0.join(name);
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(PCWSTR(shell_path(target).as_ptr())).unwrap();
            let persisted: IPersistFile = link.cast().unwrap();
            persisted
                .Save(PCWSTR(shell_path(&path).as_ptr()), true)
                .unwrap();
        }
        std::fs::canonicalize(path).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn broken_verbatim_shortcut_parses_and_recycles_link_only() {
    let fixture = Fixture::new();
    let target = fixture.0.join("removed.exe");
    let sibling = fixture.0.join("keep.txt");
    std::fs::write(&sibling, b"keep").unwrap();
    let link = fixture.link(&target, "broken.lnk");
    assert!(link.as_os_str().to_string_lossy().starts_with(r"\\?\"));
    assert_eq!(
        missing_shortcut_target(&link).unwrap(),
        Some(target.clone())
    );
    unsafe {
        let raw: Vec<u16> = link.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut pidl = std::ptr::null_mut();
        let original = SHParseDisplayName(PCWSTR(raw.as_ptr()), None, &mut pidl, 0, None);
        println!("raw verbatim SHParseDisplayName: {original:?}");
        if !pidl.is_null() {
            CoTaskMemFree(Some(pidl.cast()));
        }
        pidl = std::ptr::null_mut();
        SHParseDisplayName(PCWSTR(shell_path(&link).as_ptr()), None, &mut pidl, 0, None).unwrap();
        assert!(!pidl.is_null());
        CoTaskMemFree(Some(pidl.cast()));
    }
    recycle_broken_shortcut(&link).unwrap();
    assert!(!link.exists());
    assert!(!target.exists());
    assert_eq!(std::fs::read(&sibling).unwrap(), b"keep");
}

#[test]
fn restored_target_and_non_shortcut_are_never_deleted() {
    let fixture = Fixture::new();
    let target = fixture.0.join("restored.exe");
    let link = fixture.link(&target, "restored.LNK");
    assert!(missing_shortcut_target(&link).unwrap().is_some());
    std::fs::write(&target, b"restored").unwrap();
    assert!(missing_shortcut_target(&link).unwrap().is_none());
    assert!(
        recycle_broken_shortcut(&link)
            .unwrap_err()
            .contains("已恢复")
    );
    assert!(recycle_broken_shortcut(&target).is_err());
    assert!(link.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"restored");
}

#[test]
fn malformed_link_is_not_offered_as_a_broken_target() {
    let fixture = Fixture::new();
    let link = fixture.0.join("malformed.lnk");
    std::fs::write(&link, b"not a shortcut").unwrap();
    assert!(missing_shortcut_target(&link).is_err());
    assert!(recycle_broken_shortcut(&link).is_err());
    assert!(link.exists());
}
