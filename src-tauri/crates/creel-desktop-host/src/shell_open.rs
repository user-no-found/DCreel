use creel_ipc::HostEvent;
use creel_shell_operations::{missing_shortcut_target, shell_path};
use std::path::Path;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
    core::PCWSTR,
};

#[derive(Debug)]
pub struct OpenError {
    pub message: String,
    broken_shortcut: bool,
}
impl From<String> for OpenError {
    fn from(message: String) -> Self {
        Self {
            message,
            broken_shortcut: false,
        }
    }
}
impl OpenError {
    pub fn notification(self, path: &Path) -> HostEvent {
        let message = format!("无法打开 {}：{}", path.display(), self.message);
        if self.broken_shortcut {
            HostEvent::BrokenShortcut {
                path: path.into(),
                message,
            }
        } else {
            HostEvent::Notification { message }
        }
    }
}

/// The host UI thread already owns an OLE apartment.
pub unsafe fn open_shell_path(window: HWND, path: &Path) -> Result<(), OpenError> {
    if !path.exists() {
        return Err("文件或文件夹已经不存在".to_string().into());
    }
    if let Some(target) = missing_shortcut_target(path)? {
        return Err(OpenError {
            message: format!(
                "快捷方式指向的文件已不存在或暂时无法访问：{}",
                target.display()
            ),
            broken_shortcut: true,
        });
    }
    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let target = shell_path(path);
    let result = unsafe {
        ShellExecuteW(
            Some(window),
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as isize;
    if code <= 32 {
        Err(format!("Windows Shell 返回错误码 {code}").into())
    } else {
        Ok(())
    }
}
