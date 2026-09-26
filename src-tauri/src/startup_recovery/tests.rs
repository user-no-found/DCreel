//! `startup_recovery` 的测试：假探针驱动恢复状态机，不需要真实文件系统。

use super::*;

#[derive(Debug, Default)]
pub(crate) struct FakeProbe {
    pub(crate) present: HashMap<PathBuf, DirectoryIdentity>,
    pub(crate) indeterminate: Vec<PathBuf>,
    pub(crate) located: HashMap<String, PathBuf>,
    pub(crate) unsupported: bool,
}

impl DirectoryProbe for FakeProbe {
    fn probe(&self, path: &Path) -> ProbeOutcome {
        if self.indeterminate.iter().any(|item| item == path) {
            return ProbeOutcome::Indeterminate;
        }
        match self.present.get(path) {
            Some(identity) => ProbeOutcome::Present(identity.clone()),
            None => ProbeOutcome::Absent,
        }
    }

    fn lookup(&self, identity: &DirectoryIdentity) -> LookupOutcome {
        if self.unsupported {
            return LookupOutcome::Unsupported;
        }
        let Some(directory) = self.located.get(&identity_key(identity)).cloned() else {
            return LookupOutcome::NotFound;
        };
        let identity = self.present.get(&directory).cloned().unwrap_or_default();
        LookupOutcome::Found {
            directory,
            identity,
        }
    }
}

pub(crate) fn identity(number: u64) -> DirectoryIdentity {
    DirectoryIdentity {
        volume_guid_path: format!(r"\\?\Volume{{{number:08x}}}\"),
        volume_serial: 0x1234_5678,
        file_id_low: number,
        file_id_high: 0,
        id_kind: crate::directory_identity::DirectoryIdKind::FileId64,
        file_system: "NTFS".into(),
        creation_time: 133_000_000_000_000_000 + i64::try_from(number).unwrap(),
    }
}

pub(crate) fn fence(id: &str, directory: &str, saved: Option<DirectoryIdentity>) -> FenceConfig {
    FenceConfig {
        id: id.into(),
        title: id.into(),
        directory: PathBuf::from(directory),
        x: 0.0,
        y: 0.0,
        width: 330.0,
        height: 280.0,
        color: "coral".into(),
        content_color: "paper".into(),
        collapsed: false,
        locked: false,
        display_anchor: None,
        placement: None,
        directory_identity: saved,
    }
}

pub(crate) fn at(probe: &mut FakeProbe, path: &str, identity: DirectoryIdentity) {
    probe.present.insert(PathBuf::from(path), identity);
}

pub(crate) fn findable(probe: &mut FakeProbe, identity: &DirectoryIdentity, path: &str) {
    probe
        .located
        .insert(identity_key(identity), PathBuf::from(path));
}

#[test]
fn a_mapping_that_still_matches_is_left_alone() {
    let saved = identity(1);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\box", saved.clone());
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Intact);
    assert_eq!(fences[0].directory_identity.as_ref(), Some(&saved));
}

#[test]
fn a_renamed_directory_is_recovered_and_persisted() {
    let saved = identity(2);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\moved\box", saved.clone());
    findable(&mut probe, &saved, r"C:\moved\box");
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(
        reports[0].outcome,
        RecoveryOutcome::Relocated {
            previous: PathBuf::from(r"C:\box"),
            current: PathBuf::from(r"C:\moved\box"),
        }
    );
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\moved\box"));
    assert_eq!(fences[0].directory_identity.as_ref(), Some(&saved));
}

#[test]
fn a_same_name_replacement_does_not_steal_the_mapping() {
    let saved = identity(3);
    let impostor = identity(99);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\box", impostor.clone());
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Replaced);
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\box"));
    assert_ne!(
        fences[0].directory_identity.as_ref(),
        Some(&impostor),
        "不允许静默改绑到同名新目录"
    );
    assert_eq!(fences[0].directory_identity.as_ref(), Some(&saved));
}

#[test]
fn a_same_name_replacement_still_finds_the_original_directory() {
    let saved = identity(4);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\box", identity(100));
    at(&mut probe, r"C:\Users\Creel\Docs", saved.clone());
    findable(&mut probe, &saved, r"C:\Users\Creel\Docs");
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert!(matches!(
        reports[0].outcome,
        RecoveryOutcome::Relocated { .. }
    ));
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\Users\Creel\Docs"));
}

#[test]
fn a_recycled_file_id_with_a_new_creation_time_is_not_the_original_directory() {
    let saved = identity(18);
    // NTFS 复用 MFT 记录号：删掉重建的同名目录可能落在同一个文件 ID 上。
    let recycled = DirectoryIdentity {
        creation_time: saved.creation_time + 5_000,
        ..saved.clone()
    };
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:ox", recycled);
    let mut fences = vec![fence("a", r"C:ox", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Replaced);
    assert_eq!(fences[0].directory_identity.as_ref(), Some(&saved));
}

#[test]
fn an_unknown_creation_time_never_disqualifies_a_matching_directory() {
    let saved = DirectoryIdentity {
        creation_time: 0,
        ..identity(19)
    };
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:ox", saved.clone());
    let mut fences = vec![fence("a", r"C:ox", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Intact);
}

#[test]
fn legacy_configuration_records_the_identity_of_a_live_directory() {
    let saved = identity(5);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\box", saved.clone());
    let mut fences = vec![fence("a", r"C:\box", None)];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::IdentityRecorded);
    assert_eq!(fences[0].directory_identity, Some(saved));
}

#[test]
fn legacy_configuration_with_a_gone_directory_is_left_to_the_user() {
    let probe = FakeProbe::default();
    let mut fences = vec![fence("a", r"C:\gone", None)];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Missing);
    assert_eq!(fences[0].directory_identity, None);
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\gone"));
}

#[test]
fn an_unverified_directory_keeps_the_saved_configuration_untouched() {
    let saved = identity(6);
    let mut probe = FakeProbe::default();
    probe.indeterminate.push(PathBuf::from(r"C:\box"));
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Unverifiable);
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\box"));
    assert_eq!(fences[0].directory_identity.as_ref(), Some(&saved));
}

#[test]
fn an_unmounted_volume_reports_unsupported_instead_of_guessing() {
    let saved = identity(7);
    let probe = FakeProbe {
        unsupported: true,
        ..FakeProbe::default()
    };
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Unsupported);
    assert_eq!(fences[0].directory_identity.as_ref(), Some(&saved));
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\box"));
}

#[test]
fn a_recovery_that_would_duplicate_another_box_is_rejected() {
    let first = identity(8);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\shared", first.clone());
    findable(&mut probe, &first, r"C:\shared");
    let mut fences = vec![
        // a 的找回结果会撞上 b 正在使用的目录。
        fence("a", r"C:\renamed", Some(first.clone())),
        fence("b", r"C:\shared", None),
    ];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Conflicted);
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\renamed"));
    assert_eq!(fences[0].directory_identity.as_ref(), Some(&first));
    assert_eq!(
        reports[1].outcome,
        RecoveryOutcome::IdentityRecorded,
        "已经在正确位置上的盒子不受别人找回失败的影响"
    );
    assert_eq!(fences[1].directory, PathBuf::from(r"C:\shared"));
}

#[test]
fn two_boxes_that_truly_swap_directories_are_both_recovered() {
    let left = identity(10);
    let right = identity(11);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\left", right.clone());
    at(&mut probe, r"C:\right", left.clone());
    findable(&mut probe, &left, r"C:\right");
    findable(&mut probe, &right, r"C:\left");
    let mut fences = vec![
        fence("a", r"C:\left", Some(left.clone())),
        fence("b", r"C:\right", Some(right.clone())),
    ];

    let reports = recover_fences(&mut fences, &probe);

    assert!(matches!(
        reports[0].outcome,
        RecoveryOutcome::Relocated { .. }
    ));
    assert!(matches!(
        reports[1].outcome,
        RecoveryOutcome::Relocated { .. }
    ));
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\right"));
    assert_eq!(fences[1].directory, PathBuf::from(r"C:\left"));
}

#[test]
fn a_chained_rename_releases_the_path_the_other_box_needs() {
    let first = identity(12);
    let second = identity(13);
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\second", first.clone());
    at(&mut probe, r"C:\archived", second.clone());
    findable(&mut probe, &first, r"C:\second");
    findable(&mut probe, &second, r"C:\archived");
    let mut fences = vec![
        fence("a", r"C:\first", Some(first.clone())),
        fence("b", r"C:\second", Some(second.clone())),
    ];

    let reports = recover_fences(&mut fences, &probe);

    assert!(matches!(
        reports[0].outcome,
        RecoveryOutcome::Relocated { .. }
    ));
    assert!(matches!(
        reports[1].outcome,
        RecoveryOutcome::Relocated { .. }
    ));
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\second"));
    assert_eq!(fences[1].directory, PathBuf::from(r"C:\archived"));
}

#[test]
fn a_broken_stored_identity_is_treated_as_no_identity() {
    let mut probe = FakeProbe::default();
    at(&mut probe, r"C:\box", identity(14));

    let mut fences = vec![fence(
        "a",
        r"C:\box",
        Some(DirectoryIdentity {
            file_id_low: 7,
            ..DirectoryIdentity::default()
        }),
    )];
    let reports = recover_fences(&mut fences, &probe);
    assert_eq!(reports[0].outcome, RecoveryOutcome::IdentityRecorded);
    assert_eq!(fences[0].directory_identity, Some(identity(14)));

    let mut fences = vec![fence("b", r"C:\gone", Some(DirectoryIdentity::default()))];
    let reports = recover_fences(&mut fences, &FakeProbe::default());
    assert_eq!(reports[0].outcome, RecoveryOutcome::Missing);
    assert_eq!(
        fences[0].directory_identity,
        Some(DirectoryIdentity::default()),
        "没有可用身份时不去猜测，原样保留"
    );
}

#[test]
fn path_keys_ignore_case_and_device_namespace_prefixes() {
    assert_eq!(
        path_key(Path::new(r"C:\Box")),
        path_key(Path::new(r"c:\box\"))
    );
    assert_eq!(
        path_key(Path::new(r"\\?\C:\Box")),
        path_key(Path::new(r"c:\box"))
    );
    assert_eq!(
        path_key(Path::new(r"\\?\UNC\server\share")),
        path_key(Path::new(r"\\server\share"))
    );
    assert_ne!(
        path_key(Path::new(r"C:\box")),
        path_key(Path::new(r"C:\boxes"))
    );
}

#[test]
fn relocation_is_discarded_when_the_target_cannot_be_verified() {
    let saved = identity(15);
    let mut probe = FakeProbe::default();
    // 卷说“在那里”，但重新打开那个路径时已经不是当初的目录了。
    findable(&mut probe, &saved, r"C:\moved");
    probe.indeterminate.push(PathBuf::from(r"C:\moved"));
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Missing);
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\box"));
}

#[test]
fn relocation_is_discarded_when_the_found_identity_differs() {
    let saved = identity(16);
    let mut probe = FakeProbe::default();
    probe
        .located
        .insert(identity_key(&saved), PathBuf::from(r"C:\moved"));
    at(&mut probe, r"C:\moved", identity(17));
    let mut fences = vec![fence("a", r"C:\box", Some(saved.clone()))];

    let reports = recover_fences(&mut fences, &probe);

    assert_eq!(reports[0].outcome, RecoveryOutcome::Missing);
    assert_eq!(fences[0].directory, PathBuf::from(r"C:\box"));
}
