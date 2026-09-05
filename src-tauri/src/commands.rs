use crate::{
    desktop_context_menu,
    desktop_host::DesktopHostController,
    directory_watchers,
    models::{
        Dashboard, FenceConfig, FencePatch, FenceView, NewFenceInput, Preferences, PreferencesPatch,
    },
    store::{
        AppStore, CreelError, next_position, normalize_fence, normalize_preferences,
        unique_destination,
    },
};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use uuid::Uuid;

type CommandResult<T> = Result<T, String>;
const PROJECT_REPOSITORY_URL: &str = "https://github.com/user-no-found/DCreel";

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn sync_after_persist(app: &AppHandle, operation: &str) {
    // 持久化成功就是命令成功。Desktop Host 通过监督线程异步同步，避免
    // Explorer 尚未就绪或 IPC 超时时让控制台卡住数秒并误报操作失败。
    if let Err(error) = directory_watchers::sync_from_store(app) {
        log::warn!(
            target: "directory_watcher",
            "{operation} persisted but directory watcher synchronization is pending: {error}"
        );
    }
    let _ = app.emit("creel://state-changed", ());
    if let Some(host) = app.try_state::<DesktopHostController>() {
        host.request_sync();
    }
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
    let result = create_fence(
        title,
        input.color,
        input.content_color,
        directory.clone(),
        &app,
        &store,
    );
    if result.is_err()
        && let Err(cleanup_error) = fs::remove_dir(&directory)
    {
        log::warn!(
            target: "storage_box",
            "failed to remove unused directory {} after creation error: {cleanup_error}",
            directory.display()
        );
    }
    result
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

    let directory = directory.canonicalize().map_err(command_error)?;
    if let Some(title) = mapped_fence_title(&store, &directory)? {
        return Err(format!("该文件夹已经映射为「{title}」"));
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
    let directory = directory.canonicalize().map_err(command_error)?;
    let fence = {
        let mut state = store.lock().map_err(command_error)?;
        if let Some(existing) = state
            .fences
            .iter()
            .find(|fence| paths_refer_to_same_directory(&fence.directory, &directory))
        {
            return Err(format!("该文件夹已经映射为「{}」", existing.title));
        }
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
        if let Err(error) = store.save(&state) {
            state.fences.pop();
            return Err(command_error(error));
        }
        fence
    };
    sync_after_persist(app, "盒子");
    store.fence_view(&fence).map_err(command_error)
}

#[tauri::command]
pub fn update_fence(
    id: String,
    patch: FencePatch,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<FenceView> {
    update_fence_inner(&id, patch, &app, &store)
}

pub(crate) fn update_fence_inner(
    id: &str,
    patch: FencePatch,
    app: &AppHandle,
    store: &AppStore,
) -> CommandResult<FenceView> {
    let updated = {
        let mut state = store.lock().map_err(command_error)?;
        let current_index = state
            .fences
            .iter()
            .position(|current| current.id == id)
            .ok_or_else(|| CreelError::FenceNotFound(id.to_string()).to_string())?;
        let previous = state.fences[current_index].clone();
        let mut normalized = previous.clone();
        if let Some(title) = patch.title {
            normalized.title = normalized_title(&title)?;
        }
        if let Some(color) = patch.color {
            normalized.color = color;
        }
        if let Some(content_color) = patch.content_color {
            normalized.content_color = content_color;
        }
        if let Some(collapsed) = patch.collapsed {
            normalized.collapsed = collapsed;
        }
        if let Some(locked) = patch.locked {
            normalized.locked = locked;
        }
        let normalized = normalize_fence(normalized);

        state.fences[current_index] = normalized.clone();
        if let Err(error) = store.save(&state) {
            state.fences[current_index] = previous;
            return Err(command_error(error));
        }
        normalized
    };
    sync_after_persist(app, "盒子设置");
    store.fence_view(&updated).map_err(command_error)
}

pub(crate) fn reset_fence_size_inner(
    id: &str,
    app: &AppHandle,
    store: &AppStore,
) -> CommandResult<FenceView> {
    let updated = {
        let mut state = store.lock().map_err(command_error)?;
        let index = state
            .fences
            .iter()
            .position(|fence| fence.id == id)
            .ok_or_else(|| CreelError::FenceNotFound(id.to_string()).to_string())?;
        let previous = state.fences[index].clone();
        let width = state.preferences.default_fence_width;
        let height = state.preferences.default_fence_height;
        let other_fences = state
            .fences
            .iter()
            .enumerate()
            .filter(|(other_index, _)| *other_index != index)
            .map(|(_, fence)| fence.clone())
            .collect::<Vec<_>>();
        let mut updated = previous.clone();
        let overlaps = other_fences.iter().any(|other| {
            updated.x < other.x + other.width
                && updated.x + width > other.x
                && updated.y < other.y + other.height
                && updated.y + height > other.y
        });
        if overlaps {
            (updated.x, updated.y) = next_position(&other_fences, width, height);
        }
        updated.width = width;
        updated.height = height;
        state.fences[index] = updated.clone();
        if let Err(error) = store.save(&state) {
            state.fences[index] = previous;
            return Err(command_error(error));
        }
        updated
    };
    sync_after_persist(app, "盒子大小");
    store.fence_view(&updated).map_err(command_error)
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

fn mapped_fence_title(store: &AppStore, directory: &Path) -> CommandResult<Option<String>> {
    let state = store.lock().map_err(command_error)?;
    Ok(state
        .fences
        .iter()
        .find(|fence| paths_refer_to_same_directory(&fence.directory, directory))
        .map(|fence| fence.title.clone()))
}

fn paths_refer_to_same_directory(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
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
    sync_after_persist(&app, "盒子移除");
    Ok(())
}

#[tauri::command]
pub fn update_preferences(
    patch: PreferencesPatch,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<Preferences> {
    let update_autostart = patch.start_on_boot.is_some();
    let preferences = {
        // 在同一把锁内完成外部设置、状态修改和落盘，避免两个设置请求交叉
        // 应用。前端还会对请求排队并合并高频滑块变更。
        let mut state = store.lock().map_err(command_error)?;
        let current = state.preferences.clone();
        let mut preferences = current.clone();
        patch.apply_to(&mut preferences);
        let preferences = normalize_preferences(preferences);
        let rollback = apply_external_preferences(&app, &current, &preferences, update_autostart)?;
        state.preferences = preferences.clone();
        if let Err(error) = store.save(&state) {
            state.preferences = current.clone();
            rollback_external_preferences(&app, &rollback);
            return Err(command_error(error));
        }
        preferences
    };
    sync_after_persist(&app, "设置");
    Ok(preferences)
}

fn apply_external_preferences(
    app: &AppHandle,
    current: &Preferences,
    preferences: &Preferences,
    update_autostart: bool,
) -> CommandResult<ExternalPreferencesRollback> {
    let mut rollback = ExternalPreferencesRollback::default();
    let autostart = app.autolaunch();
    let autostart_enabled = autostart.is_enabled().unwrap_or(current.start_on_boot);
    if update_autostart && preferences.start_on_boot != autostart_enabled {
        rollback.autostart = Some(autostart_enabled);
        let result = if preferences.start_on_boot {
            autostart.enable()
        } else {
            autostart.disable()
        };
        if let Err(error) = result {
            rollback_external_preferences(app, &rollback);
            return Err(command_error(error));
        }
    }
    if preferences.desktop_context_menu != current.desktop_context_menu {
        rollback.context_menu = Some(current.desktop_context_menu);
        if let Err(error) = desktop_context_menu::set_enabled(app, preferences.desktop_context_menu)
        {
            rollback_external_preferences(app, &rollback);
            return Err(error);
        }
    }
    if preferences.show_tray_icon != current.show_tray_icon {
        rollback.tray_visible = Some(current.show_tray_icon);
        if let Some(tray) = app.tray_by_id("creel-tray")
            && let Err(error) = tray.set_visible(preferences.show_tray_icon)
        {
            rollback_external_preferences(app, &rollback);
            return Err(command_error(error));
        }
    }
    Ok(rollback)
}

#[derive(Default)]
struct ExternalPreferencesRollback {
    autostart: Option<bool>,
    context_menu: Option<bool>,
    tray_visible: Option<bool>,
}

fn rollback_external_preferences(app: &AppHandle, rollback: &ExternalPreferencesRollback) {
    if let Some(visible) = rollback.tray_visible
        && let Some(tray) = app.tray_by_id("creel-tray")
        && let Err(error) = tray.set_visible(visible)
    {
        log::error!(target: "preferences", "failed to roll back tray visibility: {error}");
    }
    if let Some(enabled) = rollback.context_menu
        && let Err(error) = desktop_context_menu::set_enabled(app, enabled)
    {
        log::error!(target: "preferences", "failed to roll back context menu: {error}");
    }
    if let Some(enabled) = rollback.autostart {
        rollback_autostart(app, enabled);
    }
}

fn rollback_autostart(app: &AppHandle, enabled: bool) {
    let autostart = app.autolaunch();
    let result = if enabled {
        autostart.enable()
    } else {
        autostart.disable()
    };
    if let Err(error) = result {
        log::error!(target: "preferences", "failed to roll back autostart: {error}");
    }
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

    let plan = plan_file_moves(paths, &canonical_target)?;
    let mut completed: Vec<PlannedMove> = Vec::with_capacity(plan.len());
    for planned in plan {
        if let Err(error) = move_path(&planned.source, &planned.destination) {
            let mut rollback_errors = Vec::new();
            for moved in completed.iter().rev() {
                if let Err(rollback_error) = move_path(&moved.destination, &moved.source) {
                    rollback_errors.push(format!("{}：{rollback_error}", moved.source.display()));
                }
            }
            let _ = app.emit("creel://state-changed", ());
            if rollback_errors.is_empty() {
                return Err(format!(
                    "移动 {} 失败，已撤销本次已经移动的项目：{error}",
                    planned.source.display()
                ));
            }
            log::error!(
                target: "file_import",
                "partial rollback failed after moving {}: {}",
                planned.source.display(),
                rollback_errors.join("；")
            );
            return Err(format!(
                "移动 {} 失败，且有 {} 个项目未能自动移回原处；请查看日志：{error}",
                planned.source.display(),
                rollback_errors.len()
            ));
        }
        completed.push(planned);
    }
    let view = store.fence_view(&fence).map_err(command_error)?;
    let _ = app.emit("creel://state-changed", ());
    Ok(view)
}

#[derive(Debug)]
struct PlannedMove {
    source: PathBuf,
    destination: PathBuf,
}

fn plan_file_moves(
    paths: Vec<PathBuf>,
    canonical_target: &Path,
) -> CommandResult<Vec<PlannedMove>> {
    let mut sources = Vec::new();
    let mut source_keys = HashSet::new();
    for source in paths {
        if !source.exists() {
            return Err(format!("拖入的项目已经不存在：{}", source.display()));
        }
        let canonical_source = source.canonicalize().map_err(command_error)?;
        if canonical_source == canonical_target || canonical_target.starts_with(&canonical_source) {
            return Err("不能把文件夹移动到它自己里面".into());
        }
        if canonical_source.parent() == Some(canonical_target) {
            continue;
        }
        if source_keys.insert(path_identity_key(&canonical_source)) {
            sources.push((source, canonical_source));
        }
    }

    // 同时拖入一个文件夹及其内部文件时，只移动最外层文件夹，避免父目录
    // 先移动后让内部项目的原路径消失。
    let canonical_sources = sources
        .iter()
        .map(|(_, canonical)| canonical.clone())
        .collect::<Vec<_>>();
    sources.retain(|(_, candidate)| {
        !canonical_sources
            .iter()
            .any(|other| other != candidate && candidate.starts_with(other))
    });

    let mut reserved_destinations = HashSet::new();
    let mut plan = Vec::with_capacity(sources.len());
    for (source, _) in sources {
        let file_name = source
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("无法识别项目名称：{}", source.display()))?;
        let destination =
            unique_reserved_destination(canonical_target, &file_name, &mut reserved_destinations);
        plan.push(PlannedMove {
            source,
            destination,
        });
    }
    Ok(plan)
}

fn unique_reserved_destination(
    target: &Path,
    file_name: &str,
    reserved: &mut HashSet<String>,
) -> PathBuf {
    let mut candidate = unique_destination(target, file_name);
    if reserved.insert(path_identity_key(&candidate)) {
        return candidate;
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
        candidate = target.join(name);
        if !candidate.exists() && reserved.insert(path_identity_key(&candidate)) {
            return candidate;
        }
    }
    loop {
        candidate = target.join(format!("{stem}-{}", Uuid::new_v4()));
        if reserved.insert(path_identity_key(&candidate)) {
            return candidate;
        }
    }
}

fn path_identity_key(path: &Path) -> String {
    let value = path.to_string_lossy();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.into_owned()
    }
}

#[tauri::command]
pub fn set_desktop_visibility(
    visible: bool,
    app: AppHandle,
    store: State<'_, AppStore>,
) -> CommandResult<bool> {
    store.set_desktop_visible(visible);
    sync_after_persist(&app, "桌面盒子显隐状态");
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
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("目标项目已经存在：{}", destination.display()),
        ));
    }
    if fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    if source.is_dir() {
        copy_directory(source, destination)?;
        let staged_source = match stage_source_for_removal(source) {
            Ok(staged_source) => staged_source,
            Err(error) => {
                let _ = fs::remove_dir_all(destination);
                return Err(error);
            }
        };
        if let Err(error) = fs::remove_dir_all(&staged_source) {
            // 目标副本已经完整写入，原目录也已原子改名离开原位置。清理失败
            // 时保留暂存副本并记录，不删除完整目标，避免部分删除导致数据丢失。
            log::warn!(
                target: "file_import",
                "moved directory but could not remove staged source {}: {error}",
                staged_source.display()
            );
        }
        Ok(())
    } else {
        copy_file_exclusive(source, destination)?;
        let staged_source = match stage_source_for_removal(source) {
            Ok(staged_source) => staged_source,
            Err(error) => {
                let _ = fs::remove_file(destination);
                return Err(error);
            }
        };
        if let Err(error) = fs::remove_file(&staged_source) {
            log::warn!(
                target: "file_import",
                "moved file but could not remove staged source {}: {error}",
                staged_source.display()
            );
        }
        Ok(())
    }
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    let result = (|| -> io::Result<()> {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_directory(&entry.path(), &target)?;
            } else {
                copy_file_exclusive(&entry.path(), &target)?;
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

fn stage_source_for_removal(source: &Path) -> io::Result<PathBuf> {
    let parent = source.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("无法确定源项目的父目录：{}", source.display()),
        )
    })?;
    let name: String = source
        .file_name()
        .map(|value| value.to_string_lossy().chars().take(80).collect())
        .unwrap_or_default();
    for _ in 0..100 {
        let staged = parent.join(format!(".dcreel-moved-{}-{name}", Uuid::new_v4()));
        if staged.exists() {
            continue;
        }
        match fs::rename(source, &staged) {
            Ok(()) => return Ok(staged),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "无法创建唯一的文件移动暂存路径",
    ))
}

fn copy_file_exclusive(source: &Path, destination: &Path) -> io::Result<()> {
    let mut source_file = fs::File::open(source)?;
    let mut destination_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let result = (|| -> io::Result<()> {
        io::copy(&mut source_file, &mut destination_file)?;
        destination_file.sync_all()?;
        if let Ok(metadata) = source_file.metadata() {
            fs::set_permissions(destination, metadata.permissions())?;
        }
        Ok(())
    })();
    drop(destination_file);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_plan_reserves_distinct_names_before_moving_anything() {
        let root = std::env::temp_dir().join(format!("dcreel-import-{}", Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        let target = root.join("target");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(first.join("notes.txt"), b"one").unwrap();
        fs::write(second.join("notes.txt"), b"two").unwrap();
        let canonical_target = target.canonicalize().unwrap();

        let plan = plan_file_moves(
            vec![first.join("notes.txt"), second.join("notes.txt")],
            &canonical_target,
        )
        .unwrap();

        assert_eq!(plan.len(), 2);
        assert_ne!(plan[0].destination, plan[1].destination);
        assert!(plan.iter().all(|item| item.source.exists()));
        assert!(target.read_dir().unwrap().next().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_plan_keeps_only_the_outermost_selected_directory() {
        let root = std::env::temp_dir().join(format!("dcreel-nested-{}", Uuid::new_v4()));
        let source = root.join("source");
        let nested = source.join("nested");
        let target = root.join("target");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&target).unwrap();

        let plan = plan_file_moves(
            vec![nested, source.clone()],
            &target.canonicalize().unwrap(),
        )
        .unwrap();

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].source, source);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cross_volume_copy_helper_never_overwrites_an_existing_file() {
        let root = std::env::temp_dir().join(format!("dcreel-exclusive-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.txt");
        let destination = root.join("destination.txt");
        fs::write(&source, b"new content").unwrap();
        fs::write(&destination, b"existing content").unwrap();

        assert_eq!(
            copy_file_exclusive(&source, &destination)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing content");
        assert_eq!(fs::read(&source).unwrap(), b"new content");
        fs::remove_dir_all(root).unwrap();
    }
}
