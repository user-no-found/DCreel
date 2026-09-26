//! 恢复结果落盘、旧配置补齐与写盘失败回滚的测试：用真实 `AppStore`，
//! 但不触碰文件系统身份 API（那部分在 `windows_directory_identity::tests`）。

use super::tests::{FakeProbe, at, fence, findable, identity};
use super::{RecoveryOutcome, recover_store};
use crate::models::PersistedState;
use crate::store::AppStore;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[test]
fn recovered_mappings_reach_the_persisted_state() {
    let root = std::env::temp_dir().join(format!("dcreel-recovered-{}", Uuid::new_v4()));
    let store = AppStore::in_directory(&root).unwrap();
    let saved = identity(20);
    {
        let mut state = store.lock().unwrap();
        state.fences = vec![fence("a", r"C:\box", Some(saved.clone()))];
        store.save(&state).unwrap();
    }
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\moved\box", saved.clone());
    findable(&mut probe, &saved, r"C:\moved\box");

    let reports = recover_store(&store, &probe);

    assert!(matches!(
        reports[0].outcome,
        RecoveryOutcome::Relocated { .. }
    ));
    let on_disk: PersistedState =
        serde_json::from_slice(&fs::read(&store.config_path).unwrap()).unwrap();
    assert_eq!(on_disk.fences[0].directory, PathBuf::from(r"C:\moved\box"));
    assert_eq!(on_disk.fences[0].directory_identity, Some(saved));
    fs::remove_dir_all(root).ok();
}

#[test]
fn legacy_state_without_identities_still_loads_and_gets_backfilled() {
    let root = std::env::temp_dir().join(format!("dcreel-legacy-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("state.json");
    // 旧版本 state.json：没有 directoryIdentity 字段，version 仍是当前值。
    fs::write(
        &path,
        format!(
            r#"{{
  "version": {},
  "fences": [{{
    "id": "legacy",
    "title": "旧盒子",
    "directory": "C:\\box",
    "x": 34.0, "y": 38.0, "width": 330.0, "height": 280.0,
    "color": "coral", "contentColor": "paper",
    "collapsed": false, "locked": false
  }}],
  "preferences": {{}}
}}"#,
            crate::models::state_version()
        ),
    )
    .unwrap();
    // 用真正的启动加载器验证：旧文件既不升级版本，也不被归档忽略。
    let loaded = crate::store::load_state_with_recovery(&path).unwrap();
    assert_eq!(loaded.fences.len(), 1);
    assert_eq!(loaded.fences[0].directory_identity, None);
    assert_eq!(loaded.version, crate::models::state_version());
    assert!(
        !fs::read_dir(&root).unwrap().flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("state.unsupported.") || name.starts_with("state.invalid.")
        }),
        "旧配置不能被归档或改名"
    );

    let store = AppStore::in_directory(&root).unwrap();
    let saved = identity(21);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\box", saved.clone());
    {
        let mut state = store.lock().unwrap();
        state.fences = loaded.fences.clone();
    }

    let reports = recover_store(&store, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::IdentityRecorded);
    let on_disk: PersistedState = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(on_disk.fences[0].directory_identity, Some(saved));
    assert_eq!(on_disk.version, crate::models::state_version());
    fs::remove_dir_all(root).ok();
}

#[test]
fn a_failed_save_rolls_the_configuration_back() {
    let root = std::env::temp_dir().join(format!("dcreel-rollback-{}", Uuid::new_v4()));
    let mut store = AppStore::in_directory(&root).unwrap();
    fs::write(root.join("blocked"), b"not a directory").unwrap();
    store.config_path = root.join("blocked").join("state.json");
    let saved = identity(22);
    {
        let mut state = store.lock().unwrap();
        state.fences = vec![fence("a", r"C:\box", Some(saved.clone()))];
    }
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\moved", saved.clone());
    findable(&mut probe, &saved, r"C:\moved");

    let reports = recover_store(&store, &probe);

    assert!(reports.is_empty(), "落盘失败时不能宣称恢复了任何盒子");
    let state = store.lock().unwrap();
    assert_eq!(state.fences[0].directory, PathBuf::from(r"C:\box"));
    assert_eq!(state.fences[0].directory_identity.as_ref(), Some(&saved));
    fs::remove_dir_all(root).ok();
}

/// 端到端：真实卷上改名 → 启动恢复 → 结果写回 state.json。
/// 读不到目录身份的机器（例如临时目录在网络上）会跳过而不是伪造成功。
#[test]
fn a_renamed_folder_is_recovered_through_the_store_end_to_end() {
    let root = std::env::temp_dir().join(format!("dcreel-e2e-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mapped = root.join("项目资料");
    fs::create_dir_all(&mapped).unwrap();
    let Some(saved) = crate::directory_identity::capture_identity(&mapped) else {
        fs::remove_dir_all(root).ok();
        return;
    };
    let store = AppStore::in_directory(&root).unwrap();
    {
        let mut state = store.lock().unwrap();
        state.fences = vec![fence("a", &mapped.to_string_lossy(), Some(saved))];
    }
    let moved = root.join("改名后的项目资料");
    fs::rename(&mapped, &moved).unwrap();

    let reports = recover_store(&store, crate::directory_identity::system_probe());

    assert_eq!(reports.len(), 1);
    let recovered = store.lock().unwrap().fences[0].directory.clone();
    let on_disk: PersistedState =
        serde_json::from_slice(&fs::read(&store.config_path).unwrap()).unwrap();
    fs::remove_dir_all(root).ok();
    assert_eq!(
        recovered.to_string_lossy().to_lowercase(),
        moved.to_string_lossy().to_lowercase(),
        "内存里的映射要指向改名后的目录"
    );
    assert_eq!(
        on_disk.fences[0].directory, recovered,
        "恢复结果必须已经落盘"
    );
}
