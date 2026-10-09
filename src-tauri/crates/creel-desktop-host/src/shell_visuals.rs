pub(crate) mod bitmap;
mod fallback;

use bitmap::CachedBitmap;
pub(crate) use bitmap::ShellVisualKind;
use creel_shell_operations::shell_path;
use std::{
    path::Path,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::SIZE,
        UI::Shell::{
            IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_BIGGERSIZEOK,
            SIIGBF_ICONONLY, SIIGBF_THUMBNAILONLY,
        },
    },
    core::PCWSTR,
};

const FAILED_VISUAL_RETRY: Duration = Duration::from_secs(5);

pub(crate) struct VisualCacheEntry {
    pub(crate) bitmap: Option<CachedBitmap>,
    last_attempt: Instant,
}

impl VisualCacheEntry {
    pub(crate) fn load(path: &Path, size: u32, now: Instant) -> Self {
        Self {
            bitmap: extract_shell_visual(path, size),
            last_attempt: now,
        }
    }

    pub(crate) fn retry_due(&self, now: Instant) -> bool {
        self.bitmap.is_none()
            && now.saturating_duration_since(self.last_attempt) >= FAILED_VISUAL_RETRY
    }
}

pub(crate) fn extract_shell_visual(path: &Path, size: u32) -> Option<CachedBitmap> {
    let wide = shell_path(path);
    let requested_size = SIZE {
        cx: size as i32,
        cy: size as i32,
    };
    let factory: Result<IShellItemImageFactory, _> =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) };
    match factory {
        Ok(factory) => {
            if path.is_file() && thumbnail_candidate(path) {
                match extract_factory_bitmap(
                    &factory,
                    requested_size,
                    SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK,
                    ShellVisualKind::Thumbnail,
                ) {
                    Ok(bitmap) => return Some(bitmap),
                    Err(error) => {
                        log::debug!(target: "shell_visuals", "thumbnail unavailable path={}: {error}", path.display())
                    }
                }
            }
            match extract_factory_bitmap(
                &factory,
                requested_size,
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
                ShellVisualKind::Icon,
            ) {
                Ok(bitmap) => return Some(bitmap),
                Err(error) => {
                    log::warn!(target: "shell_visuals", "Shell image factory icon failed path={}: {error}", path.display())
                }
            }
        }
        Err(error) => log::warn!(
            target: "shell_visuals", "SHCreateItemFromParsingName failed path={} HRESULT=0x{:08X}: {error}",
            path.display(), error.code().0 as u32,
        ),
    }
    match fallback::extract_icon(path, &wide, size) {
        Ok(bitmap) => Some(bitmap),
        Err(error) => {
            log::warn!(target: "shell_visuals", "Shell icon fallback failed path={}: {error}", path.display());
            None
        }
    }
}

fn extract_factory_bitmap(
    factory: &IShellItemImageFactory,
    requested_size: SIZE,
    flags: SIIGBF,
    kind: ShellVisualKind,
) -> Result<CachedBitmap, String> {
    let bitmap = unsafe { factory.GetImage(requested_size, flags) }.map_err(|error| {
        format!(
            "GetImage({kind:?}) HRESULT=0x{:08X}: {error}",
            error.code().0 as u32
        )
    })?;
    CachedBitmap::from_handle(bitmap, kind)
}

fn thumbnail_candidate(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "jpe"
            | "gif"
            | "webp"
            | "bmp"
            | "tif"
            | "tiff"
            | "heic"
            | "heif"
            | "avif"
            | "ico"
            | "svg"
            | "psd"
            | "raw"
            | "dng"
            | "mp4"
            | "m4v"
            | "mov"
            | "mkv"
            | "avi"
            | "wmv"
            | "webm"
            | "mpg"
            | "mpeg"
            | "mp3"
            | "m4a"
            | "flac"
            | "wav"
            | "wma"
            | "ogg"
            | "pdf"
    )
}

#[cfg(test)]
mod tests;
