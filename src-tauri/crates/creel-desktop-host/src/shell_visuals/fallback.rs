use super::bitmap::{CachedBitmap, ShellVisualKind, create_bgra_bitmap, reconstructed_alpha};
use std::{
    ffi::{OsString, c_void},
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::Path,
};
use windows::{
    Win32::{
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, HBITMAP, HDC, HGDIOBJ, SelectObject,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
        },
        UI::{
            Shell::{
                SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
            },
            WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON},
        },
    },
    core::PCWSTR,
};

struct OwnedIcon(HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}

pub(super) fn extract_icon(path: &Path, wide: &[u16], size: u32) -> Result<CachedBitmap, String> {
    let flags = SHGFI_ICON | SHGFI_LARGEICON;
    let mut info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if result != 0 && !info.hIcon.0.is_null() {
        return render_icon(OwnedIcon(info.hIcon), size);
    }
    if !info.hIcon.0.is_null() {
        drop(OwnedIcon(info.hIcon));
    }

    // Per-item handlers can fail. Ask Windows for the directory/file-type icon
    // without opening the item, including for paths beyond SHGetFileInfo's limit.
    let (name, attributes) = if path.is_dir() {
        (OsString::from("folder"), FILE_ATTRIBUTE_DIRECTORY)
    } else {
        let mut name = OsString::from("file");
        if let Some(extension) = path.extension() {
            name.push(".");
            name.push(extension);
        }
        (name, FILE_ATTRIBUTE_NORMAL)
    };
    let generic: Vec<u16> = name.encode_wide().chain(Some(0)).collect();
    let mut info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(generic.as_ptr()),
            attributes,
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            flags | SHGFI_USEFILEATTRIBUTES,
        )
    };
    if result == 0 || info.hIcon.0.is_null() {
        if !info.hIcon.0.is_null() {
            drop(OwnedIcon(info.hIcon));
        }
        return Err("SHGetFileInfoW returned no item or file-type icon".into());
    }
    render_icon(OwnedIcon(info.hIcon), size)
}

struct IconSurface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    size: i32,
    length: usize,
}

impl IconSurface {
    fn new(size: u32) -> Result<Self, String> {
        let size = size.clamp(1, 256) as i32;
        let length = size as usize * size as usize * 4;
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.0.is_null() {
            return Err("CreateCompatibleDC failed for icon fallback".into());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: length as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut::<c_void>();
        let bitmap = match unsafe {
            CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        } {
            Ok(bitmap) => bitmap,
            Err(error) => {
                unsafe {
                    let _ = DeleteDC(dc);
                }
                return Err(error.to_string());
            }
        };
        if bits.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
            }
            return Err("CreateDIBSection returned no fallback pixels".into());
        }
        let previous = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
        if previous.0.is_null() || previous.0 as isize == -1 {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
            }
            return Err("SelectObject failed for icon fallback".into());
        }
        Ok(Self {
            dc,
            bitmap,
            previous,
            bits: bits.cast(),
            size,
            length,
        })
    }

    fn draw(&mut self, icon: HICON, background: u8) -> Result<Vec<u8>, String> {
        unsafe {
            std::slice::from_raw_parts_mut(self.bits, self.length).fill(background);
            DrawIconEx(
                self.dc, 0, 0, icon, self.size, self.size, 0, None, DI_NORMAL,
            )
            .map_err(|error| format!("DrawIconEx failed: {error}"))?;
            let _ = GdiFlush();
            Ok(std::slice::from_raw_parts(self.bits, self.length).to_vec())
        }
    }
}

impl Drop for IconSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

fn render_icon(icon: OwnedIcon, size: u32) -> Result<CachedBitmap, String> {
    let mut surface = IconSurface::new(size)?;
    let mut black = surface.draw(icon.0, 0)?;
    let white = surface.draw(icon.0, 255)?;
    for (black, white) in black
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(white.as_chunks::<4>().0)
    {
        let alpha = reconstructed_alpha(black, white);
        for channel in &mut black[..3] {
            *channel = (*channel).min(alpha);
        }
        black[3] = alpha;
    }
    let handle = create_bgra_bitmap(surface.size, surface.size as u32, &black)
        .ok_or_else(|| "Could not create normalized fallback bitmap".to_string())?;
    Ok(CachedBitmap {
        handle,
        width: surface.size,
        height: surface.size,
        kind: ShellVisualKind::Icon,
        uses_alpha: true,
    })
}
