use std::ffi::c_void;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
};

pub(crate) struct CachedBitmap {
    pub(crate) handle: HBITMAP,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) kind: ShellVisualKind,
    pub(crate) uses_alpha: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellVisualKind {
    Thumbnail,
    Icon,
}

impl Drop for CachedBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.handle.0));
        }
    }
}

impl CachedBitmap {
    pub(crate) fn from_handle(handle: HBITMAP, kind: ShellVisualKind) -> Result<Self, String> {
        if handle.0.is_null() {
            return Err("GetImage returned no bitmap".into());
        }
        let mut bitmap = Self {
            handle,
            width: 0,
            height: 0,
            kind,
            uses_alpha: false,
        };
        let mut details = BITMAP::default();
        let copied = unsafe {
            GetObjectW(
                HGDIOBJ(handle.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut details as *mut BITMAP).cast::<c_void>()),
            )
        };
        if copied == 0 || details.bmWidth <= 0 || details.bmHeight == 0 {
            return Err("GetObjectW returned no valid bitmap dimensions".into());
        }
        let height = details.bmHeight.unsigned_abs();
        bitmap.width = details.bmWidth;
        bitmap.height =
            i32::try_from(height).map_err(|_| "Shell bitmap height is too large".to_string())?;
        (bitmap.handle, bitmap.uses_alpha) = normalize_shell_bitmap(handle, bitmap.width, height);
        Ok(bitmap)
    }
}

pub(crate) fn reconstructed_alpha(black: &[u8], white: &[u8]) -> u8 {
    (0..3)
        .map(|channel| 255u8.saturating_sub(white[channel].saturating_sub(black[channel])))
        .max()
        .unwrap_or_default()
}

fn normalize_shell_bitmap(bitmap: HBITMAP, width: i32, height: u32) -> (HBITMAP, bool) {
    let Some(mut pixels) = read_bitmap_pixels(bitmap, width, height) else {
        return (bitmap, false);
    };
    let uses_alpha = normalize_shell_bgra(&mut pixels);
    if !uses_alpha {
        return (bitmap, false);
    }
    let Some(normalized) = create_bgra_bitmap(width, height, &pixels) else {
        return (bitmap, true);
    };
    unsafe {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    (normalized, true)
}

pub(crate) fn read_bitmap_pixels(bitmap: HBITMAP, width: i32, height: u32) -> Option<Vec<u8>> {
    let Ok(width) = u32::try_from(width) else {
        return None;
    };
    let Ok(height_i32) = i32::try_from(height) else {
        return None;
    };
    let byte_count = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())?;
    if byte_count > 64 * 1024 * 1024 {
        return None;
    }
    let Ok(image_size) = u32::try_from(byte_count) else {
        return None;
    };
    let mut pixels = vec![0_u8; byte_count];
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -height_i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: image_size,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return None;
    }
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast::<c_void>()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(dc);
    }
    (lines == height_i32).then_some(pixels)
}

pub(crate) fn normalize_shell_bgra(pixels: &mut [u8]) -> bool {
    let pixels = pixels.as_chunks_mut::<4>().0;
    let uses_alpha = pixels.iter().any(|pixel| pixel[3] != 0);
    if !uses_alpha {
        return false;
    }
    // IShellItemImageFactory 返回的 HBITMAP 在不同图标处理器之间并不完全
    // 一致：有些是预乘 Alpha，有些是直通 Alpha，还有一些会在 alpha=0 的
    // 像素里留下未初始化的 RGB。AlphaBlend 要求预乘 Alpha，后两种情况会把
    // 透明边缘画成青色、红色或黑色细条。
    let straight_alpha = pixels.iter().any(|pixel| {
        let alpha = pixel[3];
        alpha > 0 && alpha < 255 && pixel[..3].iter().any(|channel| *channel > alpha)
    });
    for pixel in pixels {
        let alpha = pixel[3];
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 && straight_alpha {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * u16::from(alpha) + 127) / 255) as u8;
            }
        }
    }
    true
}

pub(crate) fn create_bgra_bitmap(width: i32, height: u32, pixels: &[u8]) -> Option<HBITMAP> {
    let height_i32 = i32::try_from(height).ok()?;
    let expected = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if pixels.len() != expected {
        return None;
    }
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height_i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: u32::try_from(expected).ok()?,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut::<c_void>();
    let bitmap =
        unsafe { CreateDIBSection(None, &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return None;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
    }
    Some(bitmap)
}
