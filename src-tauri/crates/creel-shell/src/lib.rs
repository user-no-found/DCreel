// Win32/COM callbacks are deliberately grouped in explicit unsafe boundary functions.
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(windows)]
mod windows_extension;

#[cfg(windows)]
pub use windows_extension::{COMMAND_GUID, EXPLORER_COMMAND_CLSID};

#[cfg(not(windows))]
pub const EXPLORER_COMMAND_CLSID: &str = "{7C998A5B-2F68-4A76-9C88-7209A70F4CA0}";

#[cfg(not(windows))]
pub const COMMAND_GUID: &str = "{06E8DE01-87AE-4DB7-A685-E8E87F5F8F8B}";
