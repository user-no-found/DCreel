use std::path::Path;
use tauri::{AppHandle, Manager};

const MENU_KEY: &str = r"Software\Classes\DesktopBackground\Shell\Creel";
const DIRECTORY_COMMAND_KEY: &str = r"Software\Classes\Directory\shell\Creel.Convert";
const CREEL_APP_KEY: &str = r"Software\Creel";

pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let shell_extension = if enabled {
        Some(find_shell_extension(app, &executable)?)
    } else {
        None
    };
    set_enabled_for_executable(&executable, shell_extension.as_deref(), enabled)
        .map_err(|error| error.to_string())
}

/// 卸载器的无界面入口：无需 Tauri AppHandle 即可移除当前用户的所有 Creel
/// Explorer 菜单和 COM 注册信息。
pub fn unregister() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    set_enabled_for_executable(&executable, None, false).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn find_shell_extension(app: &AppHandle, executable: &Path) -> Result<std::path::PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(directory) = executable.parent() {
        candidates.push(directory.join("creel_shell.dll"));
    }
    if let Ok(directory) = app.path().resource_dir() {
        candidates.push(directory.join("creel_shell.dll"));
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "没有找到 creel_shell.dll；请先构建 DCreel Shell Extension".into())
}

#[cfg(not(windows))]
fn find_shell_extension(
    _app: &AppHandle,
    _executable: &Path,
) -> Result<std::path::PathBuf, String> {
    Err("DCreel Shell Extension 仅支持 Windows".into())
}

#[cfg(windows)]
fn set_enabled_for_executable(
    executable: &Path,
    shell_extension: Option<&Path>,
    enabled: bool,
) -> std::io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    if !enabled {
        delete_tree_if_present(&current_user, MENU_KEY)?;
        delete_tree_if_present(&current_user, DIRECTORY_COMMAND_KEY)?;
        delete_tree_if_present(&current_user, &class_key())?;
        delete_tree_if_present(&current_user, CREEL_APP_KEY)?;
        return Ok(());
    }

    let shell_extension = shell_extension.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "creel_shell.dll does not exist",
        )
    })?;
    // 菜单序号或命令发生变化时，create_subkey 只会覆盖同名键，不会清掉
    // 旧的子项。重建 Creel 菜单树可避免升级开发版本后出现重复命令。
    delete_tree_if_present(&current_user, MENU_KEY)?;
    let executable = executable.to_string_lossy();
    let shell_extension = shell_extension.to_string_lossy();
    let icon = format!("{executable},0");

    let (app_key, _) = current_user.create_subkey(CREEL_APP_KEY)?;
    app_key.set_value("Executable", &executable.as_ref())?;

    let (inproc, _) = current_user.create_subkey(class_key())?;
    inproc.set_value("", &shell_extension.as_ref())?;
    inproc.set_value("ThreadingModel", &"Apartment")?;

    let (directory_command, _) = current_user.create_subkey(DIRECTORY_COMMAND_KEY)?;
    directory_command.set_value("MUIVerb", &"映射为 DCreel 盒子")?;
    directory_command.set_value("Icon", &icon)?;
    directory_command.set_value(
        "ExplorerCommandHandler",
        &creel_ipc::EXPLORER_COMMAND_CLSID_TEXT,
    )?;

    let (menu, _) = current_user.create_subkey(MENU_KEY)?;
    menu.set_value("MUIVerb", &"DCreel")?;
    menu.set_value("Icon", &icon)?;
    menu.set_value("Position", &"Top")?;
    menu.set_value("SubCommands", &"")?;

    write_verb(
        &menu,
        "shell\\01open",
        "打开 DCreel",
        &icon,
        &command_line(&executable, "--show"),
    )?;
    write_verb(
        &menu,
        "shell\\02new",
        "新建收纳盒",
        &icon,
        &command_line(&executable, "--new-fence"),
    )?;
    write_verb(
        &menu,
        "shell\\03mapped",
        "新建映射盒子",
        &icon,
        &command_line(&executable, "--new-mapped-fence"),
    )?;
    write_verb(
        &menu,
        "shell\\04toggle",
        "显示/隐藏桌面盒子",
        &icon,
        &command_line(&executable, "--toggle-fences"),
    )?;
    write_verb(
        &menu,
        "shell\\05quit",
        "退出 DCreel",
        &icon,
        &command_line(&executable, creel_ipc::ARG_QUIT),
    )?;
    Ok(())
}

#[cfg(windows)]
fn class_key() -> String {
    format!(
        r"Software\Classes\CLSID\{}\InprocServer32",
        creel_ipc::EXPLORER_COMMAND_CLSID_TEXT
    )
}

#[cfg(windows)]
fn delete_tree_if_present(parent: &winreg::RegKey, path: &str) -> std::io::Result<()> {
    match parent.delete_subkey_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn write_verb(
    parent: &winreg::RegKey,
    key: &str,
    label: &str,
    icon: &str,
    command: &str,
) -> std::io::Result<()> {
    let (verb, _) = parent.create_subkey(key)?;
    verb.set_value("MUIVerb", &label)?;
    verb.set_value("Icon", &icon)?;
    let (command_key, _) = verb.create_subkey("command")?;
    command_key.set_value("", &command)?;
    Ok(())
}

#[cfg(windows)]
fn command_line(executable: &str, argument: &str) -> String {
    format!("\"{executable}\" {argument}")
}

#[cfg(not(windows))]
fn set_enabled_for_executable(
    _executable: &Path,
    _shell_extension: Option<&Path>,
    _enabled: bool,
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn command_lines_quote_paths_with_spaces() {
        assert_eq!(
            super::command_line(r"C:\Program Files\DCreel\dcreel.exe", "--show"),
            r#""C:\Program Files\DCreel\dcreel.exe" --show"#
        );
        assert_eq!(
            super::command_line(r"C:\Program Files\DCreel\dcreel.exe", creel_ipc::ARG_QUIT),
            r#""C:\Program Files\DCreel\dcreel.exe" --quit"#
        );
    }
}
