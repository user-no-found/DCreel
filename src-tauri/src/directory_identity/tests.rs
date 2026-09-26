//! `DirectoryIdentity` 的语义测试：旧配置缺字段要能解析，身份判定必须
//! 同时看卷、完整文件 ID 与创建时间，任何一项不确定都不能认目录。

use super::{DirectoryIdKind, DirectoryIdentity};
use crate::models::FenceConfig;

#[test]
fn fences_without_a_stored_identity_still_parse() {
    let fence: FenceConfig = serde_json::from_str(
        r#"{"id":"a","title":"A","directory":"C:\\box","x":1.0,"y":2.0,
            "width":330.0,"height":280.0,"color":"coral"}"#,
    )
    .unwrap();
    assert_eq!(fence.directory_identity, None);
    assert_eq!(fence.content_color, "paper");
}

#[test]
fn an_identity_must_carry_a_volume_and_a_nonzero_file_id() {
    let empty = identity_sample();
    let without_volume = DirectoryIdentity {
        volume_guid_path: String::new(),
        file_id_low: 3,
        ..identity_sample()
    };
    let low_only = DirectoryIdentity {
        file_id_low: 3,
        ..identity_sample()
    };
    let high_only = DirectoryIdentity {
        file_id_high: 3,
        ..identity_sample()
    };
    assert!(!empty.is_usable(), "全零文件 ID 不能拿去做找回");
    assert!(
        !without_volume.is_usable(),
        "没有卷标识就无从重新打开正确的卷"
    );
    assert!(low_only.is_usable());
    assert!(
        high_only.is_usable(),
        "只有高 64 位非零的 128 位 ID 依然有效"
    );
}

#[test]
fn identity_matching_needs_the_volume_full_id_and_creation_time() {
    let saved = DirectoryIdentity {
        file_id_low: 1,
        file_id_high: 2,
        creation_time: 9,
        ..identity_sample()
    };
    // 卷标识不同：盘符和卷序列号都可能重复，不能只凭 serial 认目录。
    assert!(!saved.same_directory(&DirectoryIdentity {
        volume_guid_path: r"\\?\Volume{other}\".into(),
        ..saved.clone()
    }));
    assert!(!saved.same_directory(&DirectoryIdentity {
        volume_serial: 8,
        ..saved.clone()
    }));
    // 高 64 位被截断，或删除后重建的同名目录，都算不同目录。
    assert!(!saved.same_directory(&DirectoryIdentity {
        file_id_high: 0,
        ..saved.clone()
    }));
    assert!(!saved.same_directory(&DirectoryIdentity {
        creation_time: 10,
        ..saved.clone()
    }));
    // 读不到创建时间（0）时不下结论；文件系统名称只是诊断信息。
    assert!(saved.same_directory(&DirectoryIdentity {
        creation_time: 0,
        ..saved.clone()
    }));
    assert!(saved.same_directory(&DirectoryIdentity {
        file_system: String::new(),
        ..saved.clone()
    }));
}

#[test]
fn a_stored_identity_survives_a_json_round_trip() {
    let identity = DirectoryIdentity {
        volume_guid_path: r"\\?\Volume{one}\".into(),
        volume_serial: 7,
        file_id_low: 1,
        file_id_high: 2,
        id_kind: DirectoryIdKind::ExtendedFileId128,
        file_system: "ReFS".into(),
        creation_time: 9,
    };
    let json = serde_json::to_string(&identity).unwrap();
    assert!(json.contains("\"volumeGuidPath\""));
    assert!(json.contains("\"fileIdHigh\":2"));
    assert_eq!(
        serde_json::from_str::<DirectoryIdentity>(&json).unwrap(),
        identity
    );
}

fn identity_sample() -> DirectoryIdentity {
    DirectoryIdentity {
        volume_guid_path: r"\\?\Volume{one}\".into(),
        volume_serial: 7,
        id_kind: DirectoryIdKind::ExtendedFileId128,
        file_system: "ReFS".into(),
        ..DirectoryIdentity::default()
    }
}
