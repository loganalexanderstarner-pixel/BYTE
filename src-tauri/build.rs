fn main() {
    // --- track:found begin --- the release notes of this version, built into the app for the "What's new" card (found/whatsnew.rs)
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let from = std::path::Path::new(&dir).join("..").join("docs").join("releases").join(format!("v{version}.md"));
        let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap_or_default()).join("whatsnew.md");
        // A copy of only src/ (no docs folder) gets empty notes, and the card then says nothing.
        let text = std::fs::read_to_string(&from).unwrap_or_default();
        let _ = std::fs::write(out, text);
        println!("cargo:rerun-if-changed={}", from.display());
    }
    // --- track:found end ---
    tauri_build::build()
}
