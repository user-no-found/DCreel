use crate::{
    desktop_context_menu,
    desktop_host::DesktopHostController,
    desktop_windows,
    models::{
        Dashboard, FenceConfig, FenceView, NewFenceInput, Preferences, SweepGroup, SweepPreview,
    },
    store::{
        AppStore, CreelError, dashboard_from_state, next_position, normalize_fence,
        normalize_preferences, unique_destination, unique_directory,
    },
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use uuid::Uuid;

type CommandResult<T> = Result<T, String>;
const PROJECT_REPOSITORY_URL: &str = "https://github.com/user-no-found/DCreel";

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub fn load_dashboard(app: AppHandle, store: State<'_, AppStore>) -> CommandResult<Dashboard> {
    let mut dashboard = store.dashboard().map_err(command_error)?;
    if let Ok(enabled) = app.autolaunch().is_enabled() {
        dashboard.preferences.start_on_boot = enabled;
    }
    Ok(dashboard)
}

#[tauri::command]
pub fn create_storage_box(
    input: NewFenceInput,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<FenceView> {
    let title = crate::store::safe_directory_name(&normalized_title(&input.title)?);
    let parent = input
        .directory
        .ok_or_else(|| "请先选择文件存放位置".to_string())?;
    if !parent.is_dir() {
        return Err("选择的存放位置不存在或无法访问".into());
    }
    let directory = parent.join(&title);
    if directory.exists() {
        return Err(format!("该位置已经存在「{title}」文件夹"));
    }
    fs::create_dir(&directory).map_err(command_error)?;
    create_fence(
        title,
        input.color,
        input.content_color,
        directory,
        &app,
        &store,
    )
}

#[tauri::command]
pub fn create_mapped_box(
    input: NewFenceInput,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<FenceView> {
    let title = normalized_title(&input.title)?;
    let directory = input
        .directory
        .ok_or_else(|| "请选择要映射的文件夹".to_string())?;
    if !directory.is_dir() {
        return Err("选择的文件夹不存在或无法访问".into());
    }

    create_fence(
        title,
        input.color,
        input.content_color,
        directory,
        &app,
        &store,
    )
}

fn create_fence(
    title: String,
    color: String,
    content_color: String,
    directory: PathBuf,
    app: &AppHandle,
    store: &AppStore,
) -> CommandResult<FenceView> {
    let fence = {
        let mut state = store.lock().map_err(command_error)?;
        let width = state.preferences.default_fence_width;
        let height = state.preferences.default_fence_height;
        let (x, y) = next_position(&state.fences, width, height);
        let fence = normalize_fence(FenceConfig {
            id: Uuid::new_v4().to_string(),
            title,
            directory,
            x,
            y,
            width,
            height,
            color,
            content_color,
            collapsed: false,
            locked: false,
            display_anchor: None,
            placement: None,
        });
        state.fences.push(fence.clone());
        store.save(&state).map_err(command_error)?;
        fence
    };
    desktop_windows::sync_all(app)?;
    store.fence_view(&fence).map_err(command_error)
}

#[tauri::command]
pub fn update_fence(
    fence: FenceConfig,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<()> {
    {
        let mut state = store.lock().map_err(command_error)?;
        let current_index = state
            .fences
            .iter()
            .position(|current| current.id == fence.id)
            .ok_or_else(|| CreelError::FenceNotFound(fence.id.clone()).to_string())?;
        let previous = state.fences[current_index].clone();
        // 磁盘目录不允许通过 UI 负载直接替换。所有盒子都是文件夹
        // 映射，因此重命名盒子只改显示名称，不擅自重命名原文件夹。
        let mut normalized = normalize_fence(fence);
        normalized.directory = previous.directory.clone();
        normalized.id = previous.id.clone();

        state.fences[current_index] = normalized;
        if let Err(error) = store.save(&state) {
            state.fences[current_index] = previous;
            return Err(command_error(error));
        }
    }
    desktop_windows::sync_all(&app)
}

pub fn map_folder_inner(
    path: &Path,
    app: &AppHandle,
    store: &AppStore,
) -> CommandResult<FenceView> {
    if !path.is_dir() {
        return Err("选择的文件夹不存在或无法访问".into());
    }
    let canonical_path = path.canonicalize().map_err(command_error)?;

    if let Some(existing) = store
        .lock()
        .map_err(command_error)?
        .fences
        .iter()
        .find(|fence| {
            fence
                .directory
                .canonicalize()
                .map(|directory| directory == canonical_path)
                .unwrap_or(false)
        })
        .cloned()
    {
        return store.fence_view(&existing).map_err(command_error);
    }

    let title = canonical_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .ok_or_else(|| "无法读取文件夹名称".to_string())?;
    create_fence(
        title,
        "sage".into(),
        "paper".into(),
        canonical_path,
        app,
        store,
    )
}

#[tauri::command]
pub fn remove_fence(id: String, app: AppHandle, store: State<'_, AppStore>) -> CommandResult<()> {
    {
        let mut state = store.lock().map_err(command_error)?;
        let index = state
            .fences
            .iter()
            .position(|fence| fence.id == id)
            .ok_or_else(|| CreelError::FenceNotFound(id.clone()).to_string())?;
        let removed = state.fences.remove(index);
        // 这里只移除配置条目，不删除收纳目录或其中的任何文件。
        if let Err(error) = store.save(&state) {
            state.fences.insert(index, removed);
            return Err(command_error(error));
        }
    }
    desktop_windows::close_fence(&app, &id)
}

#[tauri::command]
pub fn update_preferences(
    preferences: Preferences,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<Preferences> {
    let preferences = normalize_preferences(preferences);
    let current = store.lock().map_err(command_error)?.preferences.clone();
    let autostart = app.autolaunch();
    let autostart_enabled = autostart.is_enabled().unwrap_or(current.start_on_boot);
    if preferences.start_on_boot != autostart_enabled {
        if preferences.start_on_boot {
            autostart.enable().map_err(command_error)?;
        } else {
            autostart.disable().map_err(command_error)?;
        }
    }
    if preferences.desktop_context_menu != current.desktop_context_menu {
        desktop_context_menu::set_enabled(&app, preferences.desktop_context_menu)?;
    }
    if preferences.show_tray_icon != current.show_tray_icon
        && let Some(tray) = app.tray_by_id("creel-tray")
    {
        tray.set_visible(preferences.show_tray_icon)
            .map_err(command_error)?;
    }

    {
        let mut state = store.lock().map_err(command_error)?;
        state.preferences = preferences.clone();
        store.save(&state).map_err(command_error)?;
    }
    desktop_windows::sync_all(&app)?;
    Ok(preferences)
}

pub(crate) fn import_files_inner(
    fence_id: &str,
    paths: Vec<PathBuf>,
    app: &AppHandle,
    store: &AppStore,
) -> CommandResult<FenceView> {
    let fence = {
        let state = store.lock().map_err(command_error)?;
        state
            .fences
            .iter()
            .find(|fence| fence.id == fence_id)
            .cloned()
            .ok_or_else(|| CreelError::FenceNotFound(fence_id.to_string()).to_string())?
    };
    fs::create_dir_all(&fence.directory).map_err(command_error)?;
    let canonical_target = fence.directory.canonicalize().map_err(command_error)?;

    for source in paths {
        if !source.exists() {
            continue;
        }
        let canonical_source = source.canonicalize().map_err(command_error)?;
        if canonical_source == canonical_target || canonical_target.starts_with(&canonical_source) {
            return Err("不能把文件夹移动到它自己里面".into());
        }
        if canonical_source.parent() == Some(canonical_target.as_path()) {
            continue;
        }
        let file_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "遇到无法识别名称的文件".to_string())?;
        let destination = unique_destination(&fence.directory, file_name);
        move_path(&source, &destination).map_err(command_error)?;
    }
    let view = store.fence_view(&fence).map_err(command_error)?;
    let _ = app.emit("creel://state-changed", ());
    Ok(view)
}

#[tauri::command]
pub fn set_desktop_visibility(
    visible: bool,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<bool> {
    store.set_desktop_visible(visible);
    desktop_windows::sync_all(&app)?;
    Ok(visible)
}

#[tauri::command]
pub fn open_path(path: PathBuf) -> CommandResult<()> {
    if !path.exists() {
        return Err("文件或文件夹已不存在".into());
    }
    if path.is_dir() {
        open_directory_in_file_manager(&path).map_err(command_error)
    } else {
        opener::open(path).map_err(command_error)
    }
}

#[tauri::command]
pub fn open_project_repository() -> CommandResult<()> {
    opener::open(PROJECT_REPOSITORY_URL).map_err(command_error)
}

#[tauri::command]
pub fn set_hotkey_capture_active(
    active: bool,
    host: State<'_, DesktopHostController>,
) -> CommandResult<()> {
    host.set_hotkey_capture_active(active)
}

#[tauri::command]
pub fn reveal_path(path: PathBuf) -> CommandResult<()> {
    if !path.exists() {
        return Err("文件或文件夹已不存在".into());
    }
    reveal_in_file_manager(&path).map_err(command_error)
}

#[tauri::command]
pub fn preview_desktop_sweep() -> CommandResult<SweepPreview> {
    let desktop = dirs::desktop_dir().ok_or_else(|| "没有找到 Windows 桌面目录".to_string())?;
    sweep_preview_for(&desktop).map_err(command_error)
}

#[tauri::command]
pub fn organize_desktop(app: AppHandle, store: State<'_, AppStore>) -> CommandResult<Dashboard> {
    let desktop = dirs::desktop_dir().ok_or_else(|| "没有找到 Windows 桌面目录".to_string())?;
    let candidates = desktop_candidates(&desktop).map_err(command_error)?;
    let mut state = store.lock().map_err(command_error)?;
    for source in candidates {
        let (key, label) = classify_path(&source);
        let directory = if let Some(fence) = state.fences.iter().find(|fence| fence.title == label)
        {
            fence.directory.clone()
        } else {
            let directory = unique_directory(&desktop, label);
            fs::create_dir_all(&directory).map_err(command_error)?;
            let width = state.preferences.default_fence_width;
            let height = state.preferences.default_fence_height;
            let (x, y) = next_position(&state.fences, width, height);
            state.fences.push(FenceConfig {
                id: Uuid::new_v4().to_string(),
                title: label.into(),
                directory: directory.clone(),
                x,
                y,
                width,
                height,
                color: color_for_group(key).into(),
                content_color: "paper".into(),
                collapsed: false,
                locked: false,
                display_anchor: None,
                placement: None,
            });
            directory
        };
        fs::create_dir_all(&directory).map_err(command_error)?;
        let file_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("文件");
        let destination = unique_destination(&directory, file_name);
        move_path(&source, &destination).map_err(command_error)?;
    }
    store.save(&state).map_err(command_error)?;
    let dashboard = dashboard_from_state(&state);
    drop(state);
    desktop_windows::sync_all(&app)?;
    Ok(dashboard)
}

fn normalized_title(title: &str) -> CommandResult<String> {
    let title: String = title.trim().chars().take(80).collect();
    if title.is_empty() {
        Err("盒子名称不能为空".into())
    } else {
        Ok(title)
    }
}

#[cfg(windows)]
pub fn open_directory_in_file_manager(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
pub fn open_directory_in_file_manager(path: &Path) -> std::io::Result<()> {
    opener::open(path).map_err(std::io::Error::other)
}

fn move_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    if fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    if source.is_dir() {
        copy_directory(source, destination)?;
        fs::remove_dir_all(source)
    } else {
        fs::copy(source, destination)?;
        fs::remove_file(source)
    }
}

fn copy_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn desktop_candidates(desktop: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(desktop)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.eq_ignore_ascii_case("desktop.ini") || name.starts_with('.') {
            continue;
        }
        if entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            candidates.push(path);
        }
    }
    Ok(candidates)
}

fn sweep_preview_for(desktop: &Path) -> std::io::Result<SweepPreview> {
    let candidates = desktop_candidates(desktop)?;
    let mut groups: BTreeMap<String, SweepGroup> = BTreeMap::new();
    for path in &candidates {
        let (key, label) = classify_path(path);
        let bytes = fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let group = groups.entry(key.into()).or_insert_with(|| SweepGroup {
            key: key.into(),
            label: label.into(),
            count: 0,
            bytes: 0,
        });
        group.count += 1;
        group.bytes += bytes;
    }
    Ok(SweepPreview {
        total: candidates.len(),
        groups: groups.into_values().collect(),
    })
}

fn classify_path(path: &Path) -> (&'static str, &'static str) {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "psd" | "ai" => {
            ("images", "图片素材")
        }
        "pdf" | "doc" | "docx" | "txt" | "md" | "xls" | "xlsx" | "ppt" | "pptx" => {
            ("documents", "文档资料")
        }
        "mp3" | "wav" | "flac" | "mp4" | "mov" | "mkv" | "avi" => ("media", "音视频"),
        "zip" | "rar" | "7z" | "tar" | "gz" => ("archives", "压缩包"),
        "lnk" | "url" => ("shortcuts", "应用快捷方式"),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "json" | "toml" => {
            ("code", "代码文件")
        }
        _ => ("other", "其他文件"),
    }
}

fn color_for_group(key: &str) -> &'static str {
    match key {
        "images" => "sage",
        "documents" => "sky",
        "media" => "lilac",
        "archives" => "butter",
        "shortcuts" => "coral",
        "code" => "graphite",
        _ => "coral",
    }
}

#[cfg(windows)]
fn reveal_in_file_manager(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
fn reveal_in_file_manager(path: &Path) -> std::io::Result<()> {
    opener::open(path.parent().unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_desktop_files() {
        assert_eq!(classify_path(Path::new("photo.PNG")).0, "images");
        assert_eq!(classify_path(Path::new("notes.pdf")).0, "documents");
        assert_eq!(classify_path(Path::new("app.lnk")).0, "shortcuts");
    }
}
