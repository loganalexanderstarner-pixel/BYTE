//! Deleting something the way the system does it: into the Recycle Bin, so the person can
//! get it back (macOS has the Trash, handled in `upkeep`). Used where BYTE removes a file
//! the person made, such as a note.

use std::path::Path;

use crate::error::{AppError, AppResult};

/// Moves a file or folder to the Recycle Bin. `path` must be absolute.
pub fn move_to_recycle_bin(path: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};

    if !path.is_absolute() {
        return Err(AppError::msg("The Recycle Bin needs the full path of what to delete."));
    }
    // The shell takes backslashes only (a forward slash is read as part of the name), and it
    // wants a list of paths: each ends in a NUL and the list ends in a second one.
    let text = path.to_string_lossy().replace('/', "\\");
    let mut from: Vec<u16> = std::ffi::OsStr::new(&text).encode_wide().collect();
    from.extend([0, 0]);
    let flags = FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0 | FOF_SILENT.0;
    let mut op = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()), fFlags: flags as u16, ..Default::default() };
    // SAFETY: `op` and the buffer it points to outlive the call, and the buffer is double-NUL terminated.
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 || op.fAnyOperationsAborted.as_bool() {
        return Err(AppError::msg(format!("Windows couldn't move {} to the Recycle Bin (code {code:#x}).", path.display())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "uses the real Recycle Bin; run with --ignored"]
    fn a_file_goes_to_the_recycle_bin() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("byte-recycle-test.md");
        std::fs::write(&file, "temporary").unwrap();
        move_to_recycle_bin(&file).unwrap();
        assert!(!file.exists(), "the file should be gone from its folder");
        assert!(move_to_recycle_bin(Path::new("relative.md")).is_err(), "a relative path is refused");
    }
}
