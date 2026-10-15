//! Is a shared library installed? Some features load a library from the system when they start (the tray icon's
//! indicator library, the Vulkan loader), and a machine without it makes the program abort instead of
//! going without. Asking first lets BYTE skip the feature quietly.

/// The directories a Linux system keeps shared libraries in.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const DIRS: &[&str] = &[
    "/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/lib/x86_64-linux-gnu", "/lib/aarch64-linux-gnu", "/usr/lib64", "/usr/lib", "/lib64", "/lib", "/usr/local/lib",
];

/// Whether any of `names` (a file name such as `libvulkan.so.1`) exists in a system library directory.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn present(names: &[&str]) -> bool {
    present_in(DIRS, names)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn present_in(dirs: &[&str], names: &[&str]) -> bool {
    dirs.iter().any(|d| names.iter().any(|n| std::path::Path::new(d).join(n).exists()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_is_found_in_any_of_the_directories() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(b.path().join("libwanted.so.1"), b"").unwrap();
        let dirs = [a.path().to_str().unwrap(), b.path().to_str().unwrap()];
        assert!(present_in(&dirs, &["libother.so", "libwanted.so.1"]));
        assert!(!present_in(&dirs, &["libmissing.so.1"]));
        assert!(!present_in(&[], &["libwanted.so.1"]));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn every_linux_has_the_c_library_and_no_machine_has_this_one() {
        assert!(present(&["libc.so.6"]));
        assert!(!present(&["libbyte-does-not-exist.so.9"]));
    }
}
