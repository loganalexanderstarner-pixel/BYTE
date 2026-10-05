fn main() {
    windows_test_binaries_can_start();
    tauri_build::build()
}

/// Lets `cargo test` binaries start on Windows.
///
/// Tauri's UI crates import `TaskDialogIndirect` from comctl32.dll, which only
/// exists in Common Controls v6, and Windows only hands out v6 to an executable
/// whose manifest asks for it. The real app has that manifest (tauri-build
/// embeds it); a test binary does not, so Windows cannot resolve the import and
/// the process dies at load with STATUS_ENTRYPOINT_NOT_FOUND (0xC0000139)
/// before a single test runs. Confirmed 2026-10-04 on a real PC: byte.exe
/// embeds the manifest, the test executable imports the function and has none.
///
/// CI never saw it because the Windows job only runs `cargo check`, so no Rust
/// test in this repository has ever executed on Windows.
///
/// Delay-loading comctl32 for tests fixes it without touching the real app.
/// Embedding a manifest instead would collide with the one tauri-build already
/// puts in the binary. No test calls a task dialog, so the delayed import is
/// never resolved.
fn windows_test_binaries_can_start() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        println!("cargo:rustc-link-arg-tests=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-arg-tests=delayimp.lib");
    }
}
