#[cfg(windows)]
mod windows_probe {
    use std::{ffi::c_void, path::Path};
    use windows::{
        Win32::{
            Foundation::S_OK,
            System::{
                Com::{CoTaskMemFree, IClassFactory},
                LibraryLoader::{GetProcAddress, LoadLibraryW},
            },
            UI::Shell::{IExplorerCommand, IShellItemArray},
        },
        core::{GUID, HRESULT, Interface, PCWSTR, s},
    };

    type GetClassObject =
        unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    type CanUnloadNow = unsafe extern "system" fn() -> HRESULT;

    pub fn run(path: &Path) -> Result<(), String> {
        let wide: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }
            .map_err(|error| format!("无法加载 {}：{error}", path.display()))?;

        let get_class_object = unsafe { GetProcAddress(module, s!("DllGetClassObject")) }
            .ok_or_else(|| "DLL 没有导出 DllGetClassObject".to_string())?;
        let can_unload_now = unsafe { GetProcAddress(module, s!("DllCanUnloadNow")) }
            .ok_or_else(|| "DLL 没有导出 DllCanUnloadNow".to_string())?;
        let get_class_object: GetClassObject = unsafe { std::mem::transmute(get_class_object) };
        let can_unload_now: CanUnloadNow = unsafe { std::mem::transmute(can_unload_now) };

        let mut raw_factory = std::ptr::null_mut();
        unsafe {
            get_class_object(
                &creel_shell::EXPLORER_COMMAND_CLSID,
                &IClassFactory::IID,
                &mut raw_factory,
            )
            .ok()
            .map_err(|error| format!("DllGetClassObject 失败：{error}"))?;
        }
        if raw_factory.is_null() {
            return Err("DllGetClassObject 没有返回类工厂".into());
        }

        let factory = unsafe { IClassFactory::from_raw(raw_factory) };
        let command: IExplorerCommand = unsafe { factory.CreateInstance(None) }
            .map_err(|error| format!("IClassFactory::CreateInstance 失败：{error}"))?;
        let canonical = unsafe { command.GetCanonicalName() }
            .map_err(|error| format!("IExplorerCommand::GetCanonicalName 失败：{error}"))?;
        if canonical != creel_shell::COMMAND_GUID {
            return Err(format!("命令 GUID 不匹配：{canonical:?}"));
        }

        let raw_title = unsafe { command.GetTitle(None::<&IShellItemArray>) }
            .map_err(|error| format!("IExplorerCommand::GetTitle 失败：{error}"))?;
        let title = unsafe { raw_title.to_string() };
        unsafe {
            CoTaskMemFree(Some(raw_title.0 as *const c_void));
        }
        let title = title.map_err(|error| format!("无法读取命令标题：{error}"))?;
        if title != "映射为 DCreel 盒子" {
            return Err(format!("命令标题不匹配：{title}"));
        }

        drop(command);
        drop(factory);
        let unload = unsafe { can_unload_now() };
        if unload != S_OK {
            return Err(format!("释放 COM 对象后 DLL 仍不可卸载：{unload:?}"));
        }

        println!(
            "DCreel Shell Extension COM probe passed: {}",
            path.display()
        );
        Ok(())
    }
}

#[cfg(windows)]
fn main() {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("用法：creel-shell-probe.exe <creel_shell.dll>");
        std::process::exit(2);
    };
    if let Err(error) = windows_probe::run(Path::new(&path)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("DCreel Shell Extension probe is only available on Windows");
}

#[cfg(windows)]
use std::path::Path;
