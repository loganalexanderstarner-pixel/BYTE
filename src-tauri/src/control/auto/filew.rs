//! Writing a file for a job: only inside a folder the contract names, never over an existing file (a new name is chosen), and never
//! a kind of file that runs.

use std::path::{Path, PathBuf};

use super::contract::Contract;

const NEVER_EXT: &[&str] = &["exe", "bat", "cmd", "ps1", "psm1", "msi", "dll", "vbs", "vbe", "js", "jse", "wsf", "lnk", "scr", "com", "reg", "jar", "sh", "hta", "cpl", "msc", "appx", "msix"];
pub const MAX_CHARS: usize = 200_000;

/// Where the file goes: the requested path when it is inside an allowed folder, or the same name with " (2)", " (3)" when it exists.
pub fn plan_write(c: &Contract, requested: &str, exists: &dyn Fn(&Path) -> bool) -> Result<PathBuf, String> {
    let req = requested.trim();
    if req.is_empty() || req.chars().any(|ch| ch.is_control()) {
        return Err("no usable file path".into());
    }
    if !c.allows_folder(req) {
        return Err(format!("{req} is not inside the folders this job may use"));
    }
    // Split by hand so a Windows path reads the same on every system.
    let cut = req.rfind(['\\', '/']).map(|i| i + 1).unwrap_or(0);
    let (dir, name) = req.split_at(cut);
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') {
        return Err("that file name is not allowed".into());
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), e.to_lowercase()),
        None => (name.to_string(), String::new()),
    };
    if NEVER_EXT.contains(&ext.as_str()) {
        return Err(format!("a job never writes .{ext} files"));
    }
    let first = PathBuf::from(req);
    if !exists(&first) {
        return Ok(first);
    }
    let orig_ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    for n in 2..100 {
        let cand = if orig_ext.is_empty() { format!("{dir}{stem} ({n})") } else { format!("{dir}{stem} ({n}).{orig_ext}") };
        let cand = PathBuf::from(cand);
        if !exists(&cand) {
            return Ok(cand);
        }
    }
    Err("too many files with that name".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c() -> Contract {
        Contract { folders: vec!["C:\\Work".into()], ..Default::default() }.normalized()
    }

    #[test]
    fn a_new_file_inside_an_allowed_folder_is_fine() {
        assert_eq!(plan_write(&c(), "C:\\Work\\notes\\a.txt", &|_| false).unwrap(), PathBuf::from("C:\\Work\\notes\\a.txt"));
    }

    #[test]
    fn outside_the_folders_or_with_dotdot_it_is_refused() {
        assert!(plan_write(&c(), "C:\\Other\\a.txt", &|_| false).is_err());
        assert!(plan_write(&c(), "C:\\Work\\..\\Other\\a.txt", &|_| false).is_err());
    }

    #[test]
    fn programs_and_scripts_are_never_written() {
        for f in ["a.exe", "a.BAT", "a.ps1", "a.js", "a.lnk", "a.msi"] {
            assert!(plan_write(&c(), &format!("C:\\Work\\{f}"), &|_| false).is_err(), "{f}");
        }
    }

    #[test]
    fn an_existing_file_is_never_overwritten() {
        let taken = [PathBuf::from("C:\\Work\\a.txt"), PathBuf::from("C:\\Work\\a (2).txt")];
        let got = plan_write(&c(), "C:\\Work\\a.txt", &|p| taken.contains(&p.to_path_buf())).unwrap();
        assert_eq!(got, PathBuf::from("C:\\Work\\a (3).txt"));
    }
}
