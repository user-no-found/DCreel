use crate::models::{
    Dashboard, FenceConfig, FencePlacement, FenceView, PersistedState, Preferences, state_version,
};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum CreelError {
    #[error("无法读取或写入本地文件：{0}")]
    Io(#[from] std::io::Error),
    #[error("配置文件格式无效：{0}")]
    Json(#[from] serde_json::Error),
    #[error("没有找到盒子：{0}")]
    FenceNotFound(String),
    #[error("DCreel 的内部状态暂时不可用")]
    StatePoisoned,
}

pub type CreelResult<T> = Result<T, CreelError>;

pub struct AppStore {
    pub inner: Mutex<PersistedState>,
    pub config_path: PathBuf,
    desktop_visible: AtomicBool,
}

impl AppStore {
    pub fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        // 显式覆盖只用于隔离的开发/集成测试；正常启动始终使用系统应用配置目录。
        let root_dir = std::env::var_os("CREEL_STATE_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or(app.path().app_config_dir()?);
        fs::create_dir_all(&root_dir)?;
        cleanup_stale_state_temps(&root_dir);
        let config_path = root_dir.join("state.json");
        let state = load_state_with_recovery(&config_path)?;

        Ok(Self {
            inner: Mutex::new(state),
            config_path,
            desktop_visible: AtomicBool::new(true),
        })
    }

    pub fn lock(&self) -> CreelResult<MutexGuard<'_, PersistedState>> {
        self.inner.lock().map_err(|_| CreelError::StatePoisoned)
    }

    pub fn save(&self, state: &PersistedState) -> CreelResult<()> {
        write_state(&self.config_path, state)
    }

    pub fn dashboard(&self) -> CreelResult<Dashboard> {
        let state = self.lock()?.clone();
        Ok(dashboard_from_state(&state, self.desktop_visible()))
    }

    pub fn fence_view(&self, fence: &FenceConfig) -> CreelResult<FenceView> {
        let show_hidden = self.lock()?.preferences.show_hidden_files;
        Ok(fence_to_view(fence.clone(), show_hidden))
    }

    pub fn desktop_visible(&self) -> bool {
        self.desktop_visible.load(Ordering::Relaxed)
    }

    pub fn set_desktop_visible(&self, visible: bool) {
        self.desktop_visible.store(visible, Ordering::Relaxed);
    }
}

fn cleanup_stale_state_temps(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".state.")
            && name.ends_with(".tmp")
            && let Err(error) = fs::remove_file(entry.path())
        {
            log::warn!(
                target: "state",
                "failed to remove stale state temporary file {}: {error}",
                entry.path().display()
            );
        }
    }
}

pub fn dashboard_from_state(state: &PersistedState, desktop_visible: bool) -> Dashboard {
    let fences = state
        .fences
        .iter()
        .cloned()
        .map(|fence| fence_to_view(fence, state.preferences.show_hidden_files))
        .collect();
    Dashboard {
        fences,
        preferences: state.preferences.clone(),
        desktop_visible,
    }
}

fn fence_to_view(config: FenceConfig, show_hidden: bool) -> FenceView {
    let item_count = count_directory_items(&config.directory, show_hidden).unwrap_or_default();
    FenceView { config, item_count }
}

fn count_directory_items(directory: &Path, show_hidden: bool) -> CreelResult<usize> {
    if !directory.is_dir() {
        return Ok(0);
    }

    let mut count = 0usize;
    for entry in fs::read_dir(directory)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        if !show_hidden && is_hidden(&entry.path(), &name) {
            continue;
        }
        count = count.saturating_add(1);
    }
    Ok(count)
}

#[cfg(windows)]
fn is_hidden(path: &Path, name: &str) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    name.starts_with('.')
        || fs::metadata(path)
            .map(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
            .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_hidden(_path: &Path, name: &str) -> bool {
    name.starts_with('.')
}

pub fn normalize_preferences(mut preferences: Preferences) -> Preferences {
    preferences.title_opacity = normalized_opacity(preferences.title_opacity, 0.9);
    preferences.content_opacity = normalized_opacity(preferences.content_opacity, 0.9);
    preferences.fence_border_opacity = normalized_opacity(preferences.fence_border_opacity, 0.4);
    preferences.icon_size = preferences.icon_size.clamp(36, 64);
    preferences.default_fence_width = bounded_geometry_value(
        preferences.default_fence_width,
        244.0,
        800.0,
        Preferences::default().default_fence_width,
    );
    preferences.default_fence_height = bounded_geometry_value(
        preferences.default_fence_height,
        148.0,
        700.0,
        Preferences::default().default_fence_height,
    );
    preferences.ghost_opacity = if preferences.ghost_opacity.is_finite() {
        preferences.ghost_opacity.clamp(0.0, 1.0)
    } else {
        Preferences::default().ghost_opacity
    };
    preferences.ghost_hotkey = preferences.ghost_hotkey.trim().chars().take(64).collect();
    if preferences.ghost_hotkey.is_empty() {
        preferences.ghost_hotkey = "Ctrl+Alt+G".into();
    }
    preferences.ignored_update_version = preferences
        .ignored_update_version
        .map(|version| version.trim().chars().take(64).collect::<String>())
        .filter(|version| !version.is_empty());
    preferences
}

fn normalized_opacity(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

pub fn normalize_fence(mut fence: FenceConfig) -> FenceConfig {
    fence.title = fence.title.trim().chars().take(80).collect();
    if fence.title.is_empty() {
        fence.title = "未命名盒子".into();
    }
    // Windows 虚拟桌面允许副显示器位于主屏左侧或上方，因此坐标可以为负数。
    fence.x = fence.x.clamp(-32_768.0, 32_768.0);
    fence.y = fence.y.clamp(-32_768.0, 32_768.0);
    fence.width = fence.width.clamp(244.0, 1600.0);
    fence.height = fence.height.clamp(148.0, 1200.0);
    fence.color = normalize_color_value(&fence.color, "coral", false);
    fence.content_color = normalize_color_value(&fence.content_color, "paper", true);
    fence.placement = fence.placement.and_then(normalize_placement);
    fence
}

fn normalize_color_value(value: &str, fallback: &str, allow_materials: bool) -> String {
    let value = value.trim();
    let named = matches!(
        value,
        "coral" | "sage" | "butter" | "sky" | "lilac" | "graphite"
    ) || allow_materials && matches!(value, "paper" | "frosted");
    if named {
        return value.into();
    }
    if value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return value.to_ascii_uppercase();
    }
    fallback.into()
}

fn normalize_placement(mut placement: FencePlacement) -> Option<FencePlacement> {
    placement.group_id = placement.group_id.trim().chars().take(128).collect();
    if placement.group_id.is_empty() {
        return None;
    }
    for axis in [&mut placement.horizontal, &mut placement.vertical] {
        if !axis.value.is_finite() {
            return None;
        }
        axis.value = match axis.anchor {
            creel_ipc::LayoutAnchor::Start | creel_ipc::LayoutAnchor::End => {
                axis.value.clamp(0.0, 65_536.0)
            }
            creel_ipc::LayoutAnchor::Proportional => axis.value.clamp(0.0, 1.0),
        };
    }
    if !placement.offset_x_dip.is_finite() || !placement.offset_y_dip.is_finite() {
        return None;
    }
    placement.offset_x_dip = placement.offset_x_dip.clamp(0.0, 65_536.0);
    placement.offset_y_dip = placement.offset_y_dip.clamp(0.0, 65_536.0);
    Some(placement)
}

pub fn apply_fence_geometry(
    fence: &mut FenceConfig,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> bool {
    let next_x = bounded_geometry_value(x, -32_768.0, 32_768.0, fence.x);
    let next_y = bounded_geometry_value(y, -32_768.0, 32_768.0, fence.y);
    let next_width = bounded_geometry_value(width, 244.0, 1_600.0, fence.width);
    // 收起时原生窗口高度固定为 48px，保留展开时的持久化高度。
    let next_height = if fence.collapsed {
        fence.height
    } else {
        bounded_geometry_value(height, 148.0, 1_200.0, fence.height)
    };
    let changed = fence.x != next_x
        || fence.y != next_y
        || fence.width != next_width
        || fence.height != next_height;
    fence.x = next_x;
    fence.y = next_y;
    fence.width = next_width;
    fence.height = next_height;
    changed
}

fn bounded_geometry_value(value: f64, minimum: f64, maximum: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback
    }
}

pub fn next_position(fences: &[FenceConfig], width: f64, height: f64) -> (f64, f64) {
    let width = width.clamp(244.0, 800.0);
    let height = height.clamp(148.0, 700.0);
    let column_step = width + 26.0;
    let row_step = height + 26.0;

    for index in 0..10_000 {
        let column = index % 3;
        let row = index / 3;
        let x = 34.0 + column as f64 * column_step;
        let y = 38.0 + row as f64 * row_step;
        let overlaps = fences.iter().any(|fence| {
            x < fence.x + fence.width
                && x + width > fence.x
                && y < fence.y + fence.height
                && y + height > fence.y
        });
        if !overlaps {
            return (x, y);
        }
    }

    let row = fences.len() / 3;
    (34.0, 38.0 + row as f64 * row_step)
}

pub fn safe_directory_name(title: &str) -> String {
    let name: String = title
        .trim()
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            value if value.is_control() => ' ',
            value => value,
        })
        .collect();
    let name = name.trim().trim_end_matches('.');
    if name.is_empty() {
        format!("收纳盒-{}", &Uuid::new_v4().to_string()[..8])
    } else {
        name.chars().take(60).collect()
    }
}

pub fn unique_destination(target_dir: &Path, file_name: &str) -> PathBuf {
    let initial = target_dir.join(file_name);
    if !initial.exists() {
        return initial;
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("文件");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 2..10_000 {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = target_dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    target_dir.join(format!("{stem}-{}", Uuid::new_v4()))
}

fn load_current_state(path: &Path) -> CreelResult<Option<PersistedState>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read(path)?;
    let document: serde_json::Value = serde_json::from_slice(&content)?;
    let is_current = document
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|version| version == u64::from(state_version()));
    if !is_current {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&content)?))
}

fn load_state_with_recovery(path: &Path) -> CreelResult<PersistedState> {
    match load_current_state(path) {
        Ok(Some(state)) => return Ok(state),
        Ok(None) if !path.exists() => {
            if let Some(state) = load_backup_state(path) {
                log::warn!(
                    target: "state",
                    "state.json is missing; restored the last valid backup"
                );
                write_state(path, &state)?;
                return Ok(state);
            }
        }
        Ok(None) => {
            let archived = quarantine_state(path, "unsupported")?;
            log::warn!(
                target: "state",
                "unsupported state version preserved at {}",
                archived.display()
            );
            if let Some(state) = load_backup_state(path) {
                log::warn!(target: "state", "restored the last compatible state backup");
                write_state(path, &state)?;
                return Ok(state);
            }
        }
        Err(error) => {
            let archived = quarantine_state(path, "invalid")?;
            log::error!(
                target: "state",
                "invalid state preserved at {}: {error}",
                archived.display()
            );
            if let Some(state) = load_backup_state(path) {
                log::warn!(target: "state", "restored state.json from the last valid backup");
                write_state(path, &state)?;
                return Ok(state);
            }
        }
    }

    // 新状态不预设任何桌面文件夹或示例盒子。无法兼容或恢复的旧文件
    // 已被保留为旁路副本，不再静默删除用户数据。
    let fresh = PersistedState::default();
    write_state(path, &fresh)?;
    Ok(fresh)
}

fn load_backup_state(path: &Path) -> Option<PersistedState> {
    let backup = state_backup_path(path);
    match load_current_state(&backup) {
        Ok(Some(state)) => Some(state),
        Ok(None) => None,
        Err(error) => {
            log::error!(
                target: "state",
                "state backup is invalid and was left untouched at {}: {error}",
                backup.display()
            );
            None
        }
    }
}

fn quarantine_state(path: &Path, reason: &str) -> CreelResult<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let archived = parent.join(format!(
        "state.{reason}.{timestamp}.{}.json",
        &Uuid::new_v4().to_string()[..8]
    ));
    fs::rename(path, &archived)?;
    Ok(archived)
}

fn state_backup_path(path: &Path) -> PathBuf {
    path.with_file_name("state.backup.json")
}

fn write_state(path: &Path, state: &PersistedState) -> CreelResult<()> {
    let bytes = serde_json::to_vec_pretty(state)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".state.{}.tmp", Uuid::new_v4()));
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        replace_state_file(&temporary, path, &state_backup_path(path))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(CreelError::Io)
}

#[cfg(windows)]
fn replace_state_file(temporary: &Path, target: &Path, backup: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_WRITE_THROUGH, ReplaceFileW,
        },
        core::PCWSTR,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let temporary = wide(temporary);
    let target_wide = wide(target);
    if target.exists() {
        match fs::remove_file(backup) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let backup = wide(backup);
        unsafe {
            ReplaceFileW(
                PCWSTR(target_wide.as_ptr()),
                PCWSTR(temporary.as_ptr()),
                PCWSTR(backup.as_ptr()),
                REPLACEFILE_WRITE_THROUGH,
                None,
                None,
            )
            .map_err(|_| io::Error::last_os_error())
        }
    } else {
        unsafe {
            MoveFileExW(
                PCWSTR(temporary.as_ptr()),
                PCWSTR(target_wide.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|_| io::Error::last_os_error())
        }
    }
}

#[cfg(not(windows))]
fn replace_state_file(temporary: &Path, target: &Path, backup: &Path) -> io::Result<()> {
    if target.exists() {
        let backup_temporary = backup.with_extension(format!("tmp-{}", Uuid::new_v4()));
        fs::copy(target, &backup_temporary)?;
        fs::rename(&backup_temporary, backup)?;
    }
    fs::rename(temporary, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_names_drop_windows_reserved_characters() {
        assert_eq!(safe_directory_name("  项目:A/B?  "), "项目 A B");
    }

    #[test]
    fn preferences_are_clamped() {
        let result = normalize_preferences(Preferences {
            title_opacity: 4.0,
            content_opacity: -2.0,
            fence_border_opacity: f64::NAN,
            icon_size: 2,
            ghost_opacity: -3.0,
            ..Preferences::default()
        });
        assert_eq!(result.title_opacity, 1.0);
        assert_eq!(result.content_opacity, 0.0);
        assert_eq!(result.fence_border_opacity, 0.4);
        assert_eq!(result.icon_size, 36);
        assert_eq!(result.ghost_opacity, 0.0);

        let non_finite = normalize_preferences(Preferences {
            ghost_opacity: f64::NAN,
            ..Preferences::default()
        });
        assert_eq!(non_finite.ghost_opacity, 0.2);
    }

    #[test]
    fn fence_colors_accept_arbitrary_hex_and_content_materials() {
        assert_eq!(
            normalize_color_value(" #12abEf ", "coral", false),
            "#12ABEF"
        );
        assert_eq!(normalize_color_value("#0f8342", "paper", true), "#0F8342");
        assert_eq!(normalize_color_value("paper", "paper", true), "paper");
        assert_eq!(normalize_color_value("frosted", "paper", true), "frosted");
        assert_eq!(normalize_color_value("frosted", "coral", false), "coral");
        assert_eq!(normalize_color_value("#12XZ89", "paper", true), "paper");
    }

    #[test]
    fn new_fence_position_skips_occupied_boxes() {
        let occupied = FenceConfig {
            id: "occupied".into(),
            title: "Occupied".into(),
            directory: PathBuf::from(r"C:\occupied"),
            x: 34.0,
            y: 38.0,
            width: 330.0,
            height: 280.0,
            color: "coral".into(),
            content_color: "paper".into(),
            collapsed: false,
            locked: false,
            display_anchor: None,
            placement: None,
        };
        assert_eq!(next_position(&[], 330.0, 280.0), (34.0, 38.0));
        assert_eq!(next_position(&[occupied], 330.0, 280.0), (390.0, 38.0));
    }

    #[test]
    fn host_geometry_is_clamped_and_collapsed_height_is_preserved() {
        let mut fence = FenceConfig {
            id: "geometry-test".into(),
            title: "几何测试".into(),
            directory: PathBuf::from(r"C:\geometry-test"),
            x: 20.0,
            y: 30.0,
            width: 330.0,
            height: 280.0,
            color: "coral".into(),
            content_color: "paper".into(),
            collapsed: true,
            locked: false,
            display_anchor: None,
            placement: None,
        };

        assert!(apply_fence_geometry(
            &mut fence, -90_000.0, 90_000.0, 100.0, 48.0
        ));
        assert_eq!(fence.x, -32_768.0);
        assert_eq!(fence.y, 32_768.0);
        assert_eq!(fence.width, 244.0);
        assert_eq!(fence.height, 280.0);
        assert!(!apply_fence_geometry(
            &mut fence,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            900.0
        ));
    }

    #[test]
    fn state_writes_keep_the_previous_complete_document_as_backup() {
        let directory = std::env::temp_dir().join(format!("dcreel-state-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("state.json");
        let mut first = PersistedState::default();
        first.preferences.title_opacity = 0.35;
        write_state(&path, &first).unwrap();

        let mut second = first.clone();
        second.preferences.title_opacity = 0.8;
        write_state(&path, &second).unwrap();

        assert_eq!(
            load_current_state(&path)
                .unwrap()
                .unwrap()
                .preferences
                .title_opacity,
            0.8
        );
        assert_eq!(
            load_current_state(&state_backup_path(&path))
                .unwrap()
                .unwrap()
                .preferences
                .title_opacity,
            0.35
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_state_is_quarantined_and_restored_from_backup() {
        let directory = std::env::temp_dir().join(format!("dcreel-recovery-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("state.json");
        let mut backed_up = PersistedState::default();
        backed_up.preferences.content_opacity = 0.42;
        write_state(&path, &backed_up).unwrap();
        let mut latest = backed_up.clone();
        latest.preferences.content_opacity = 0.9;
        write_state(&path, &latest).unwrap();
        fs::write(&path, b"{ definitely not valid json").unwrap();

        let recovered = load_state_with_recovery(&path).unwrap();
        assert_eq!(recovered.preferences.content_opacity, 0.42);
        assert!(directory.read_dir().unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("state.invalid.")
        }));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dashboard_item_count_is_exact_beyond_the_old_five_hundred_limit() {
        let directory = std::env::temp_dir().join(format!("dcreel-count-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        for index in 0..503 {
            fs::write(directory.join(format!("item-{index:04}.txt")), b"").unwrap();
        }

        assert_eq!(count_directory_items(&directory, true).unwrap(), 503);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dashboard_uses_the_authoritative_runtime_visibility() {
        let state = PersistedState::default();
        assert!(!dashboard_from_state(&state, false).desktop_visible);
        assert!(dashboard_from_state(&state, true).desktop_visible);
    }
}
