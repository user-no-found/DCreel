//! Shared Shell boundary helpers. Stored filesystem paths are never rewritten.
#[cfg(windows)]
mod paths;
#[cfg(windows)]
mod shortcuts;
#[cfg(windows)]
pub use paths::shell_path;
#[cfg(windows)]
pub use shortcuts::{missing_shortcut_target, recycle_broken_shortcut};

#[cfg(all(test, windows))]
mod tests;
