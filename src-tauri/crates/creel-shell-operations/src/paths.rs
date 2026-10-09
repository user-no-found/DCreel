use std::{os::windows::ffi::OsStrExt, path::Path};

/// Convert filesystem verbatim spellings to paths understood by Shell parsers.
/// Preserve UTF-16 and change only drive/UNC spellings at the Shell boundary.
pub fn shell_path(path: &Path) -> Vec<u16> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    let unc: Vec<u16> = r"UNC\".encode_utf16().collect();
    let mut shell = if wide.starts_with(&verbatim) {
        let tail = &wide[verbatim.len()..];
        if tail.len() >= unc.len()
            && tail[..unc.len()]
                .iter()
                .zip(&unc)
                .all(|(actual, expected)| {
                    *actual == *expected || actual.checked_sub(32) == Some(*expected)
                })
        {
            let mut value = vec![b'\\' as u16, b'\\' as u16];
            value.extend_from_slice(&tail[unc.len()..]);
            value
        } else if tail.len() >= 3
            && tail[1] == b':' as u16
            && tail[2] == b'\\' as u16
            && ((b'A' as u16..=b'Z' as u16).contains(&tail[0])
                || (b'a' as u16..=b'z' as u16).contains(&tail[0]))
        {
            tail.to_vec()
        } else {
            wide
        }
    } else {
        wide
    };
    shell.push(0);
    shell
}
