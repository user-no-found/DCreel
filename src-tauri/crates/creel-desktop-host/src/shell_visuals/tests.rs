use super::{VisualCacheEntry, bitmap, extract_shell_visual, fallback, thumbnail_candidate};
use creel_shell_operations::shell_path;
use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::SIZE,
        System::Ole::{OleInitialize, OleUninitialize},
        UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY},
    },
    core::PCWSTR,
};

struct TestApartment;

impl TestApartment {
    fn initialize() -> Self {
        unsafe { OleInitialize(None) }.expect("test OLE apartment should initialize");
        Self
    }
}

impl Drop for TestApartment {
    fn drop(&mut self) {
        unsafe { OleUninitialize() };
    }
}

fn terminated(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn project_image() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("workspace root should exist")
        .join("Creel .png")
}

#[test]
fn shell_paths_accept_verbatim_drive_and_unc_paths_without_changing_other_namespaces() {
    assert_eq!(
        shell_path(Path::new(r"\\?\C:\code\Ã©Â¡Â¹Ã§â€ºÂ®")),
        terminated(r"C:\code\Ã©Â¡Â¹Ã§â€ºÂ®")
    );
    assert_eq!(
        shell_path(Path::new(r"\\?\UNC\server\share\file.txt")),
        terminated(r"\\server\share\file.txt")
    );
    assert_eq!(
        shell_path(Path::new(r"\\?\unc\server\share")),
        terminated(r"\\server\share")
    );
    assert_eq!(shell_path(Path::new(r"C:\code")), terminated(r"C:\code"));
    assert_eq!(
        shell_path(Path::new(r"\\?\Volume{example}\folder")),
        terminated(r"\\?\Volume{example}\folder")
    );
}

#[test]
fn shell_path_conversion_preserves_non_unicode_windows_names() {
    let mut original: Vec<u16> = r"\\?\C:\code\".encode_utf16().collect();
    original.push(0xd800);
    let path = PathBuf::from(OsString::from_wide(&original));
    let mut expected = original[4..].to_vec();
    expected.push(0);
    assert_eq!(shell_path(&path), expected);
}

#[test]
fn windows_image_factory_reads_canonical_mapped_directory_icons() {
    let _apartment = TestApartment::initialize();
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    let wide = shell_path(&folder);
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
            .expect("canonical mapped folder should parse in Shell");
    let handle = unsafe { factory.GetImage(SIZE { cx: 48, cy: 48 }, SIIGBF_ICONONLY) }
        .expect("mapped folder should have a Shell icon");
    let image = bitmap::CachedBitmap::from_handle(handle, bitmap::ShellVisualKind::Icon).unwrap();
    assert!(image.width > 0 && image.height > 0);
    assert!(extract_shell_visual(&folder, 48).is_some());
}

#[test]
fn windows_shell_exposes_a_real_bitmap_for_the_canonical_host_binary() {
    let _apartment = TestApartment::initialize();
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let image = extract_shell_visual(&executable, 48).expect("executable icon should exist");
    assert_eq!(image.kind, bitmap::ShellVisualKind::Icon);
    assert!(image.width > 0 && image.height > 0);
    if image.uses_alpha {
        let pixels =
            bitmap::read_bitmap_pixels(image.handle, image.width, image.height.unsigned_abs())
                .unwrap();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] != 0 || pixel[..3] == [0, 0, 0])
        );
    }
}

#[test]
fn windows_shell_exposes_canonical_image_thumbnails() {
    let _apartment = TestApartment::initialize();
    let image = extract_shell_visual(&project_image().canonicalize().unwrap(), 64)
        .expect("Windows Shell should expose the project PNG thumbnail");
    assert_eq!(image.kind, bitmap::ShellVisualKind::Thumbnail);
    assert!(image.width > 0 && image.height > 0);
}

#[test]
fn fallback_reads_real_folder_icons_with_transparent_backgrounds() {
    let _apartment = TestApartment::initialize();
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    let image = fallback::extract_icon(&folder, &shell_path(&folder), 48).unwrap();
    assert_eq!(image.kind, bitmap::ShellVisualKind::Icon);
    assert!(image.uses_alpha);
    let pixels =
        bitmap::read_bitmap_pixels(image.handle, image.width, image.height.unsigned_abs()).unwrap();
    assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
    assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0));
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3].iter().all(|channel| *channel <= pixel[3]))
    );
}

#[test]
fn fallback_supplies_a_file_type_icon_when_the_item_cannot_be_opened() {
    let _apartment = TestApartment::initialize();
    let missing = std::env::temp_dir().join("creel-definitely-missing-visual-file.txt");
    let image = fallback::extract_icon(&missing, &shell_path(&missing), 48).unwrap();
    assert_eq!(image.kind, bitmap::ShellVisualKind::Icon);
    assert_eq!((image.width, image.height), (48, 48));
}

#[test]
fn failed_visuals_retry_after_backoff_and_successful_visuals_stay_cached() {
    let now = Instant::now();
    let failed = VisualCacheEntry {
        bitmap: None,
        last_attempt: now,
    };
    assert!(!failed.retry_due(now + Duration::from_secs(4)));
    assert!(failed.retry_due(now + Duration::from_secs(5)));
    let _apartment = TestApartment::initialize();
    let successful = VisualCacheEntry::load(Path::new(env!("CARGO_MANIFEST_DIR")), 48, now);
    assert!(successful.bitmap.is_some());
    assert!(!successful.retry_due(now + Duration::from_secs(600)));
}

#[test]
fn shell_bitmap_alpha_normalization_clears_garbage_and_premultiplies_straight_edges() {
    let mut straight = vec![255, 64, 32, 0, 200, 100, 50, 128, 30, 20, 10, 255];
    assert!(bitmap::normalize_shell_bgra(&mut straight));
    assert_eq!(&straight[0..4], &[0, 0, 0, 0]);
    assert_eq!(&straight[4..8], &[100, 50, 25, 128]);
    assert_eq!(&straight[8..12], &[30, 20, 10, 255]);
    let mut premultiplied = vec![100, 50, 25, 128, 9, 8, 7, 0];
    assert!(bitmap::normalize_shell_bgra(&mut premultiplied));
    assert_eq!(&premultiplied[0..4], &[100, 50, 25, 128]);
    assert_eq!(&premultiplied[4..8], &[0, 0, 0, 0]);
}

#[test]
fn black_white_icon_passes_recover_transparent_and_partial_alpha() {
    assert_eq!(
        bitmap::reconstructed_alpha(&[0, 0, 0, 0], &[255, 255, 255, 0]),
        0
    );
    assert_eq!(
        bitmap::reconstructed_alpha(&[0, 32, 128, 0], &[127, 159, 255, 0]),
        128
    );
    assert_eq!(
        bitmap::reconstructed_alpha(&[10, 20, 30, 0], &[10, 20, 30, 0]),
        255
    );
}

#[test]
fn thumbnail_extensions_are_case_insensitive() {
    assert!(thumbnail_candidate(Path::new("poster.JPEG")));
    assert!(thumbnail_candidate(Path::new("clip.MP4")));
    assert!(thumbnail_candidate(Path::new("document.PDF")));
    assert!(!thumbnail_candidate(Path::new("notes.txt")));
}
