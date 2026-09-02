use std::{
    ffi::c_void,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use windows::{
    Win32::{
        Foundation::{
            CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, E_POINTER,
            E_UNEXPECTED, S_FALSE, S_OK,
        },
        System::Com::{CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl},
        UI::Shell::{
            ECF_DEFAULT, ECS_ENABLED, ECS_HIDDEN, IEnumExplorerCommand, IExplorerCommand,
            IExplorerCommand_Impl, IShellItemArray, SHStrDupW, SIGDN_FILESYSPATH,
        },
    },
    core::{Error, GUID, HRESULT, Interface, PCWSTR, PWSTR, Ref, implement},
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

pub const EXPLORER_COMMAND_CLSID: GUID = GUID::from_u128(0x7c998a5b_2f68_4a76_9c88_7209a70f4ca0);
pub const COMMAND_GUID: GUID = GUID::from_u128(0x06e8de01_87ae_4db7_a685_e8e87f5f8f8b);

const CREEL_REGISTRY_KEY: &str = r"Software\Creel";
static ACTIVE_OBJECTS: AtomicUsize = AtomicUsize::new(0);
static SERVER_LOCKS: AtomicUsize = AtomicUsize::new(0);

struct ServerObjectGuard;

impl ServerObjectGuard {
    fn new() -> Self {
        ACTIVE_OBJECTS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for ServerObjectGuard {
    fn drop(&mut self) {
        ACTIVE_OBJECTS.fetch_sub(1, Ordering::Release);
    }
}

#[implement(IExplorerCommand)]
struct CreelExplorerCommand {
    _guard: ServerObjectGuard,
}

impl CreelExplorerCommand {
    fn new() -> Self {
        Self {
            _guard: ServerObjectGuard::new(),
        }
    }
}

impl IExplorerCommand_Impl for CreelExplorerCommand_Impl {
    fn GetTitle(&self, _items: Ref<'_, IShellItemArray>) -> windows::core::Result<PWSTR> {
        duplicate_string("映射为 DCreel 盒子")
    }

    fn GetIcon(&self, _items: Ref<'_, IShellItemArray>) -> windows::core::Result<PWSTR> {
        let icon = executable_path()
            .map(|path| format!("{},0", path.display()))
            .unwrap_or_default();
        duplicate_string(&icon)
    }

    fn GetToolTip(&self, _items: Ref<'_, IShellItemArray>) -> windows::core::Result<PWSTR> {
        duplicate_string("把这个文件夹映射为 DCreel 多宫格盒子")
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(COMMAND_GUID)
    }

    fn GetState(
        &self,
        items: Ref<'_, IShellItemArray>,
        _ok_to_be_slow: windows::core::BOOL,
    ) -> windows::core::Result<u32> {
        let enabled = selected_folder(items)
            .map(|path| path.is_dir())
            .unwrap_or(false);
        Ok(if enabled {
            ECS_ENABLED.0 as u32
        } else {
            ECS_HIDDEN.0 as u32
        })
    }

    fn Invoke(
        &self,
        items: Ref<'_, IShellItemArray>,
        _bind_context: Ref<'_, IBindCtx>,
    ) -> windows::core::Result<()> {
        let folder = selected_folder(items)?;
        if !folder.is_dir() {
            return Err(Error::new(E_FAIL, "请选择一个可访问的文件夹"));
        }
        let executable = executable_path()?;
        Command::new(executable)
            .args(creel_ipc::map_folder_args(folder))
            .spawn()
            .map(|_| ())
            .map_err(|error| windows_error("无法启动 DCreel", error))
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

#[implement(IClassFactory)]
struct CreelClassFactory {
    _guard: ServerObjectGuard,
}

impl CreelClassFactory {
    fn new() -> Self {
        Self {
            _guard: ServerObjectGuard::new(),
        }
    }
}

impl IClassFactory_Impl for CreelClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<'_, windows::core::IUnknown>,
        iid: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if !outer.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if iid.is_null() || object.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            object.write(std::ptr::null_mut());
        }
        let command: IExplorerCommand = CreelExplorerCommand::new().into();
        unsafe { command.query(iid, object).ok() }
    }

    fn LockServer(&self, lock: windows::core::BOOL) -> windows::core::Result<()> {
        if lock.as_bool() {
            SERVER_LOCKS.fetch_add(1, Ordering::Relaxed);
        } else {
            let _ = SERVER_LOCKS.fetch_update(Ordering::Release, Ordering::Relaxed, |count| {
                count.checked_sub(1)
            });
        }
        Ok(())
    }
}

fn duplicate_string(value: &str) -> windows::core::Result<PWSTR> {
    let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    unsafe { SHStrDupW(PCWSTR(wide.as_ptr())) }
}

fn selected_folder(items: Ref<'_, IShellItemArray>) -> windows::core::Result<PathBuf> {
    let items = items.ok()?;
    let count = unsafe { items.GetCount()? };
    if count != 1 {
        return Err(Error::new(E_FAIL, "请选择一个文件夹"));
    }
    let item = unsafe { items.GetItemAt(0)? };
    let raw_path = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH)? };
    let path = unsafe { raw_path.to_string() };
    unsafe {
        CoTaskMemFree(Some(raw_path.0 as *const c_void));
    }
    Ok(PathBuf::from(path?))
}

fn executable_path() -> windows::core::Result<PathBuf> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let key = current_user
        .open_subkey(CREEL_REGISTRY_KEY)
        .map_err(|error| windows_error("没有找到 DCreel 注册信息", error))?;
    let value: String = key
        .get_value("Executable")
        .map_err(|error| windows_error("没有找到 DCreel 可执行文件", error))?;
    let path = PathBuf::from(value);
    if path.is_file() {
        Ok(path)
    } else {
        Err(Error::new(E_FAIL, "DCreel 可执行文件不存在"))
    }
}

fn windows_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::new(E_FAIL, format!("{context}：{error}"))
}

fn catch_hresult(action: impl FnOnce() -> HRESULT) -> HRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(result) => result,
        Err(_) => E_UNEXPECTED,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    catch_hresult(|| {
        if object.is_null() || clsid.is_null() || iid.is_null() {
            return E_POINTER;
        }
        unsafe {
            object.write(std::ptr::null_mut());
            if clsid.read() != EXPLORER_COMMAND_CLSID {
                return CLASS_E_CLASSNOTAVAILABLE;
            }
        }
        let factory: IClassFactory = CreelClassFactory::new().into();
        unsafe { factory.query(iid, object) }
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    catch_hresult(|| {
        if ACTIVE_OBJECTS.load(Ordering::Acquire) == 0 && SERVER_LOCKS.load(Ordering::Acquire) == 0
        {
            S_OK
        } else {
            S_FALSE
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unload_status_tracks_live_com_objects() {
        assert_eq!(DllCanUnloadNow(), S_OK);
        let command: IExplorerCommand = CreelExplorerCommand::new().into();
        assert_eq!(DllCanUnloadNow(), S_FALSE);
        drop(command);
        assert_eq!(DllCanUnloadNow(), S_OK);
    }
}
