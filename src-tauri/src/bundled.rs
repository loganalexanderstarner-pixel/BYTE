//! Starting BYTE's bundled tools (llama-server, whisper-cli, the sherpa tools)
//! on every platform, including Android (docs/ANDROID.md, A1).
//!
//! On desktops `llama-server`, `whisper-cli` and the sherpa tools are Tauri
//! sidecars. Android only lets an app run programs from its own native library
//! folder (W^X), and only files named `lib*.so` are installed there, so each
//! tool ships as `lib<name>.so` (`scripts/build-llama-android.sh`) and is started
//! from that folder. Everything after the start (arguments, the HTTP API, health
//! checks, logs) is the same code as on the desktop.

use std::path::{Path, PathBuf};

use tauri::AppHandle;
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::ShellExt;

/// The command that runs one of BYTE's bundled tools ("llama-server", …).
pub fn tool(app: &AppHandle, name: &str) -> Result<Command, String> {
    if cfg!(target_os = "android") {
        let dir = native_dir().ok_or("BYTE couldn't find its own native library folder")?;
        let path = pick(&dir, name, cpu_has_i8mm()).ok_or_else(|| format!("{name} isn't in this build"))?;
        Ok(app.shell().command(path))
    } else {
        app.shell().sidecar(name).map_err(|e| e.to_string())
    }
}

/// The installed program for `name`: the i8mm build of llama-server when the CPU
/// has it (much faster prompt reading), otherwise the baseline build.
fn pick(dir: &Path, name: &str, i8mm: bool) -> Option<PathBuf> {
    let fast = dir.join(format!("lib{name}-i8mm.so"));
    let base = dir.join(format!("lib{name}.so"));
    if i8mm && fast.is_file() {
        Some(fast)
    } else {
        base.is_file().then_some(base)
    }
}

/// The app's native library folder: where Android put `libbyte_lib.so`, found
/// in this process's own memory map (no Java call needed).
fn native_dir() -> Option<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    lib_dir_in_maps(&maps)
}

fn lib_dir_in_maps(maps: &str) -> Option<PathBuf> {
    maps.lines()
        .filter_map(|l| l.split_whitespace().nth(5))
        .find(|p| p.ends_with("/libbyte_lib.so"))
        .and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
}

pub fn cpu_has_i8mm() -> bool {
    std::fs::read_to_string("/proc/cpuinfo").map(|c| has_i8mm(&c)).unwrap_or(false)
}

/// Every core must have it: llama-server's threads may run on any of them.
fn has_i8mm(cpuinfo: &str) -> bool {
    let features: Vec<&str> = cpuinfo.lines().filter(|l| l.starts_with("Features")).collect();
    !features.is_empty() && features.iter().all(|l| l.split_whitespace().any(|w| w == "i8mm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_native_library_folder() {
        let maps = "7f00-7f10 r--p 00000000 fd:00 1 /system/lib64/libc.so\n\
                    7a00-7a80 r-xp 00000000 fd:01 2 /data/app/~~x==/com.loganstarner.byte-y==/lib/arm64/libbyte_lib.so\n";
        assert_eq!(lib_dir_in_maps(maps), Some(PathBuf::from("/data/app/~~x==/com.loganstarner.byte-y==/lib/arm64")));
        assert_eq!(lib_dir_in_maps("7f00-7f10 r--p 0 fd:00 1 /system/lib64/libc.so\n"), None);
    }

    #[test]
    fn i8mm_only_when_every_core_has_it() {
        let big_little = "processor : 0\nFeatures : fp asimd dotprod\nprocessor : 7\nFeatures : fp asimd dotprod i8mm bf16\n";
        assert!(!has_i8mm(big_little));
        assert!(has_i8mm("Features : fp asimd i8mm\nFeatures : fp i8mm sve2\n"));
        assert!(!has_i8mm("model name : something\n"));
    }

    #[test]
    fn picks_the_fast_build_when_it_can() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(pick(dir.path(), "llama-server", true), None);
        std::fs::write(dir.path().join("libllama-server.so"), b"").unwrap();
        assert_eq!(pick(dir.path(), "llama-server", true).unwrap().file_name().unwrap(), "libllama-server.so");
        std::fs::write(dir.path().join("libllama-server-i8mm.so"), b"").unwrap();
        assert_eq!(pick(dir.path(), "llama-server", true).unwrap().file_name().unwrap(), "libllama-server-i8mm.so");
        assert_eq!(pick(dir.path(), "llama-server", false).unwrap().file_name().unwrap(), "libllama-server.so");
    }
}
