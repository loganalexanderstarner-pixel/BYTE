# Porting BYTE to Windows and Linux — brief for the Claude session on the owner's PC

You are a Claude Code session with SSH access to Logan's PC. Your job is to make BYTE run natively on
**Windows first, then Linux**, on that PC. Another Claude session (in the cloud) keeps working on the Mac app on
the same repo. Read this file, then `CLAUDE.md`, `docs/HANDOFF.md` (§1b has the order of work) and
`docs/DESIGN-AND-PLATFORMS.md` Part 2 (the platform plan) before writing code.

## The machine

| | |
|---|---|
| CPU | AMD Ryzen 7 7800X3D: 8 cores / 16 threads, Zen 4 (AVX2 and AVX-512) |
| RAM | 32 GB DDR5-6000, dual channel (about 90 GB/s: what CPU-offloaded layers run at) |
| GPU | NVIDIA RTX 5080, 16 GB GDDR7 (about 960 GB/s). Blackwell, compute capability **12.0 (sm_120)**: needs **CUDA 12.8 or newer** |
| OS | Dual boot: Windows (do this first) and Linux (after). Check the Linux distro with `cat /etc/os-release` |

This PC is far faster than the target Mac for anything that fits in 16 GB of VRAM. Use it to test the
"any hardware" planner: models that fit in VRAM, models split between VRAM and RAM, and CPU-only.

## Ground rules

- **Branches.** Don't push to `claude/new-session-tu1a5x` (the Mac session releases from it, and a push during a
  release breaks publishing). Work on **`claude/windows-port`** (later `claude/linux-port`), branched from it.
  Merge `claude/new-session-tu1a5x` into yours often; don't rebase or force-push. When a milestone works, say
  so in your report, and the owner will have the branches merged.
- **Never break the Mac.** Platform code goes behind `#[cfg(target_os = "windows")]` / `"linux"`, with a clear
  fallback, like `ocr.rs` already does. Shared modules stay platform-free. The `windows` job in `ci.yml`
  already compiles the Rust core for Windows on every push: keep it green.
- **Conventions are in `CLAUDE.md`:** Rust owns side effects; tests next to code; log every commit in
  `docs/WORKLOG.md` (what, why, files, verify, undo); `scripts/check-all.sh` before pushing (on Windows, run its
  steps by hand or in Git Bash).
- **No secrets in the repo, ever** (it's public). Never ask Logan to paste a key into a chat.
- **Each OS should feel native** (owner decision): not "macOS-shaped". The UI is one React app, but how BYTE talks
  to the OS, where it keeps things and how it installs follow Windows and Linux conventions.
- **Honest limits:** never claim a model runs when it won't.

## Setting up the PC

**Windows**
- Visual Studio 2022 Build Tools (workload "Desktop development with C++"), Rust (`rustup`, MSVC toolchain),
  Node 22, Git, CMake and Ninja.
- CUDA Toolkit 12.8+, plus the Vulkan SDK (for the Vulkan engine build). WebView2 is built into Windows 11.
- `git clone https://github.com/loganalexanderstarner-pixel/BYTE`, then
  `git checkout -b claude/windows-port origin/claude/new-session-tu1a5x`, then `npm ci`.

**Linux** (later)
- Tauri's packages: on Ubuntu/Debian `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev
  librsvg2-dev libsoup-3.0-dev libdbus-1-dev`, plus `build-essential cmake ninja-build`.
- The NVIDIA driver and the CUDA toolkit (12.8+), Rust and Node 22.

## What exists, and what's Mac-only

- **Engine.** `scripts/build-llama-server.sh` builds llama.cpp (tag pinned in `scripts/LLAMA_TAG`) with Metal.
  `whisper-cli` and the two sherpa tools are built by `build-whisper.sh` / `build-sherpa.sh`. Tauri expects
  sidecars at `src-tauri/binaries/<name>-<target-triple>[.exe]` (see `tauri.conf.json` `externalBin`). The
  Linux x86_64 sidecars already build in the cloud container, so Linux is mostly done for the engine.
- **Memory planner.** `system.rs` assumes Apple's unified memory (one pool shared by CPU and GPU). A PC has VRAM
  and RAM separately. `system::plan_offload` already splits layers and MoE experts between GPU and CPU: grow it
  into the per-device plan in DESIGN-AND-PLATFORMS ("Best automatically" / GPU only / CPU only / Use both, via
  `-ngl`, `--tensor-split`, `--main-gpu`, `--n-cpu-moe`).
- **Files with `#[cfg(target_os = "macos")]`** (most to least), each needing a native version or a clear
  "not on Windows yet": `system.rs`, `wake.rs`, `ocr.rs`, `lock.rs`, `tts.rs`, `speech.rs`, `files.rs`,
  `connectors/mod.rs`, `clipboard.rs`, `quick.rs`, `cloud/keychain.rs`, `web_agent/browser.rs`, `voice.rs`,
  `shortcut_make.rs`, `privacy.rs`, `notes.rs`, `memory.rs`, `lib.rs`, and one each in `upkeep.rs`, `terminal.rs`,
  `selection.rs`, `media.rs`, `macctl.rs`, `kb.rs`, `filectl.rs`, `dashboard.rs`, `chip.rs`, `briefing.rs`,
  `automations.rs`, `agent.rs`, `web_agent/mod.rs`.
- **Shell-outs to macOS tools** (`osascript`, `afconvert`, `afplay`, `say`, `pmset`, `mdfind`, `xattr` …) are mostly
  in `macctl.rs` (16), `system.rs` (11), `terminal.rs`, `upkeep.rs`, `speech.rs`, `wake.rs`, `voice.rs`.

## The work, in milestones

Report to Logan after each one: what works, screenshots, numbers, what's next.

### W1. Windows: BYTE builds, runs, and answers on the GPU
1. **Engine build for Windows.** Recommended: llama.cpp with **dynamic backends**
   (`-DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DGGML_CUDA=ON -DGGML_VULKAN=ON`), so one `llama-server`
   picks CUDA, Vulkan or the best CPU code at runtime: NVIDIA, AMD and Intel GPUs and CPU-only PCs all work.
   CUDA archs must include `120` (5080) and the common older ones (`75;80;86;89;120`). Check that the CUDA
   runtime DLLs it needs ship next to it (they're redistributable).
   - Write `scripts/build-llama-server.ps1`, or extend the `.sh` for Git Bash. The same goes for whisper and sherpa
     (CUDA or Vulkan for whisper; CPU is fine for sherpa).
2. **GPU and VRAM detection** (`system.rs`, `chip.rs`): NVIDIA through NVML or `nvidia-smi --query-gpu`; other GPUs
   through DXGI adapters. Fill `SystemInfo` with GPU name, VRAM and RAM. Make the planner VRAM-aware: on this PC,
   a 9B Q6 model should fit entirely in VRAM with a long context.
3. `npm run tauri dev` works; the welcome guide picks a model for "RTX 5080 16 GB + 32 GB"; a chat answers.
   Record tokens per second (Settings → Engine's speed test) for a few models.
4. **Real-engine tests:** `cargo test` natively, plus `BYTE_TEST_LLAMA_SERVER=… BYTE_TEST_MODEL=… cargo test e2e
   -- --ignored`.

### W2. Windows: the core features feel native
- **Secrets:** `cloud/keychain.rs` and connector secrets → Windows Credential Manager (the `keyring` crate covers
  Windows and Linux).
- **Lock:** `lock.rs` → Windows Hello (`Windows.Security.Credentials.UI.UserConsentVerifier`), falling back to the
  Windows sign-in.
- **OCR:** `ocr.rs` → `Windows.Media.Ocr`.
- **Spoken answers:** the sherpa voices (`tts.rs`) are cross-platform, so check they work. `speech.rs`'s `say`
  → Windows' built-in voices (SAPI / `Windows.Media.SpeechSynthesis`).
- **Voice:** the mic through `cpal` (`wake.rs`) should work as is; audio conversion (`afconvert`) → do it in Rust or
  with a bundled tool.
- **Quick Ask, tray and shortcuts** (`quick.rs`): the taskbar tray and global shortcuts; check them on Windows.
- **Clipboard history, selection, files:** Windows equivalents.
- **Open at login:** the Run key or Task Scheduler.

### W3. Windows: OS control and upkeep
- `macctl.rs` (Notes, Reminders, Calendar, Mail, Music…) → the Windows way: PowerShell and COM (Outlook,
  Office), UI Automation, toast notifications with actions, Windows calendar/to-do where reachable.
  The same approval card and Undo apply.
- `upkeep.rs` / `filectl.rs` / `terminal.rs`: storage cleanup (the Recycle Bin, not delete), battery, PowerShell
  instead of zsh.
- Wherever Windows allows *more* than macOS (owner's wish), note ideas in your report.

### W4. Windows: packaging, updates, release
- An NSIS installer (per-user, no admin) is the Tauri default. Unsigned is fine: SmartScreen shows "More info →
  Run anyway" (see DESIGN-AND-PLATFORMS "Signing").
- **Updates:** the same signing key (GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD`; checked by
  `scripts/updater-key.sh`). `latest.json` must list `windows-x86_64` next to `darwin-aarch64`, so the release
  becomes a matrix (macOS + Windows jobs on the same tag; tauri-action merges `latest.json`). Coordinate this
  change with the Mac session, because it touches `release.yml`.
- GitHub's Windows runners have no GPU: CI builds and unit tests only. Real GPU testing is on this PC.

### L1–L4. Linux, the same way
- CUDA (and Vulkan) engine builds; AppImage and `.deb`.
- Secrets → Secret Service (`keyring`); OCR → Tesseract; lock → polkit or the desktop's own authentication.
- Tray, autostart (`~/.config/autostart`), systemd user timers, and `xdotool`/`ydotool` or D-Bus for app
  control. Check both X11 and Wayland if the distro uses Wayland.

## Reporting

After each milestone, write to Logan in plain words: what you tried, what works (with screenshots), speed
numbers, what doesn't yet and why. Log commits in `docs/WORKLOG.md`. Update `docs/HANDOFF.md` §2's table with
a Windows and a Linux row.
