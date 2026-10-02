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

## Any hardware: every GPU vendor, and the user's choice of memory

BYTE on Windows and Linux must work well on **any PC**: NVIDIA, **AMD** and **Intel** graphics (dedicated cards and
integrated graphics), several GPUs, or no usable GPU at all. It must be **as smart about the hardware as the Mac
app is**. Logan's 5080 is the test machine, not the target: test the other paths too (Vulkan works on the 5080 and
the Ryzen's integrated Radeon; CPU-only with the GPU switched off).

### What the Mac app already does (match all of it)
- **Knows the hardware:** `chip.rs` identifies the exact chip and its memory bandwidth and GPU compute, so BYTE can
  **estimate speed before a download** (generation is bound by memory bandwidth, reading the prompt by compute).
- **Plans memory honestly:** `system.rs` works out what each model needs (weights + context cache + overhead),
  gives every catalog version a fit (great / fits / tight / won't fit), shrinks the context when memory is tight,
  and never offers a model that won't run (`gpu_budget`, `plan_offload`).
- **Measures real speed:** `speed.rs` measures writing and prompt-reading speed per model on this machine; cards
  show measured numbers once known.
- **Tunes itself per machine and model:** `tune.rs` (quick tune the first time a model loads; thorough tune and
  "tune all" in Settings → Engine). It tries Speed boost (draft models, MTP/EAGLE heads), KV-cache precision, batch
  sizes and flash attention, measures each, and keeps the fastest. `modelcfg.rs` sets each model family's
  recommended sampling and thinking control.
- **Recommends per machine:** the welcome guide and Settings → Models pick the best model for this hardware.
  "Fits alongside" says which models can load next to the main one. Battery saver applies on laptops.

### What the PC version adds
1. **Detects every GPU and its memory:**
   - **NVIDIA** through NVML: name, VRAM total and free, compute capability, memory bandwidth (from the clock and
     bus width), temperature.
   - **AMD** through ROCm-SMI / amdgpu sysfs on Linux, and DXGI + ADL/ADLX on Windows.
   - **Intel** (Arc and integrated) through DXGI on Windows, and Level Zero / sysfs on Linux.
   - On every OS, Vulkan device enumeration is a vendor-neutral fallback.

   Also the **CPU** (cores, AVX2/AVX-512, cache) and **system RAM** (size, and speed and channels from SMBIOS where
   readable, which bounds CPU-offload speed). Keep a small built-in table like `chip.rs` (GPU model → bandwidth /
   compute) for estimates, and correct it with measurements.
2. **Picks the best engine backend per device:**
   - CUDA on NVIDIA;
   - **ROCm/HIP or Vulkan on AMD** (ROCm where supported, Vulkan everywhere else);
   - **Vulkan on Intel** (SYCL optional later);
   - the fastest CPU variant otherwise.

   With dynamic backends (`GGML_BACKEND_DL`), one `llama-server` holds them all. The tuner can also *measure* CUDA
   vs Vulkan on NVIDIA and keep the faster.
3. **Memory modes, the user's choice** (Settings → Engine, per model, with "Best automatically" as the default):
   - **Best automatically:** fill VRAM with as many layers as fit (leaving room for the context cache and the
     desktop), put the rest in system RAM on the CPU. MoE models keep shared layers on the GPU and push experts to
     RAM first (`--n-cpu-moe` / `-ot`), which keeps them fast.
   - **GPU only:** everything in VRAM; models that don't fit are marked "won't fit on the GPU".
   - **CPU and RAM only:** no GPU (for a busy GPU or a gaming session); speed estimated from RAM bandwidth.
   - **Use both, my split:** a slider for how much goes on the GPU vs the CPU (`-ngl`), and with several GPUs, how to
     split between them (`--tensor-split`, `--main-gpu`).

   This is how **bigger models** run: on Logan's PC, 16 GB VRAM + 32 GB RAM together can run models far larger
   than either alone (for example a 30B–70B model, or a large MoE with experts in RAM). The cards say what each mode
   means for speed ("about 6 words/s split across GPU and RAM" vs "won't fit on the GPU alone").
4. **Fit and estimates per mode:** every catalog version shows its fit and an estimated speed **for the chosen
   mode** on this PC (GPU-only, split, CPU-only), with the same rule as the Mac: never say it runs when it won't.
   "Fits alongside" counts VRAM and RAM separately.
5. **Tuning on PC hardware:** the tuner also tries the GPU/CPU split (more or fewer layers on the GPU), the backend
   (CUDA vs Vulkan), thread count (physical cores, not hyperthreads), KV precision, flash attention, batch sizes and
   Speed boost, and keeps what's fastest *on this PC for this model*.
6. **Several GPUs:** detect them all; let the user choose one, or split a model across them.
7. **Keeps working when things change:** VRAM freed or taken by other apps (games), a laptop on battery, or a
   driver without CUDA. Re-plan instead of crashing, and say what changed.

Test matrix to report: on the 5080 (CUDA, then Vulkan), the Ryzen's integrated graphics (Vulkan), and CPU-only,
for a small, a medium and a large model (which needs the split), with measured speeds per mode.

## Feature map: every Mac feature, and what Windows and Linux do instead

How to use it: build the **Windows** and **Linux** columns. "Same" means the existing cross-platform code should
just work; check it. When something truly can't exist on an OS, BYTE says so plainly and offers the closest thing,
never a broken button. Everything that changes something keeps the Mac rules: an approval card first, Undo where
possible, a line in the activity log.

Prefer local OS APIs that need no account. Anything that needs a Microsoft or Google sign-in (Graph, Google
Calendar API) is an optional connector for later; ask Logan before adding one.

### Mac features → Windows → Linux

| Mac feature (where) | Windows | Linux |
|---|---|---|
| AI engine on Metal (`engine.rs`, `build-llama-server.sh`) | llama.cpp CUDA + Vulkan + CPU (dynamic backends) | Same build: CUDA / Vulkan / ROCm / CPU |
| Memory planner, unified memory (`system.rs`) | VRAM + RAM planner: GPU only / CPU only / split / best automatically | Same as Windows |
| Touch ID lock (`lock.rs`) | **Windows Hello** (face, fingerprint or PIN) | polkit / PAM: fingerprint through `fprintd` if present, else the account password |
| Keychain for keys (`cloud/keychain.rs`, connectors) | Credential Manager (`keyring` crate) | Secret Service / GNOME Keyring / KWallet (`keyring`) |
| Text in photos and scans, Apple Vision (`ocr.rs`) | `Windows.Media.Ocr` (built in, offline) | Tesseract (bundled or a package) |
| Mac voices, `say` (`speech.rs`) | Windows voices (`Windows.Media.SpeechSynthesis`, natural voices) | speech-dispatcher / espeak-ng; BYTE's sherpa voices are better and cross-platform |
| BYTE's voices, sherpa (`tts.rs`, `voices.rs`) | Same | Same |
| Dictation and "Hey BYTE", whisper + cpal mic (`voice.rs`, `wake.rs`) | Same (WASAPI under cpal); whisper on CUDA | Same (PipeWire/ALSA under cpal); whisper on CUDA |
| Audio conversion, `afconvert` (`voice.rs`, `speakers.rs`) | In Rust (`symphonia`) or a bundled ffmpeg | Same as Windows |
| Menu-bar icon (`quick.rs`) | System tray icon + **taskbar jump list** (New chat, Quick Ask, Lock) | Tray (AppIndicator / StatusNotifierItem); GNOME needs the AppIndicator extension, so say so |
| Quick Ask ⌥Space, global shortcuts (`quick.rs`) | Same (Alt+Space is taken by Windows; default to Ctrl+Space or Win+Shift+B) | Same; Wayland may need the desktop's portal for global shortcuts |
| Ask about selected text (`selection.rs`) | UI Automation to read the selection (fallback: simulate Ctrl+C) | The **PRIMARY selection** (highlighted text, no copy needed): `wl-paste -p` / `xclip -o` |
| Clipboard history (`clipboard.rs`) | Same, alongside Windows' own Win+V history | Same (`wl-clipboard` / X11) |
| Notes, Markdown files (`notes.rs`) | Same, in Documents\\BYTE\\Notes; "Show in File Explorer" | Same, in ~/Documents/BYTE/Notes (XDG dirs) |
| Apple Notes (`macctl.rs`) | OneNote desktop through COM if installed; otherwise BYTE's own Notes | BYTE's own Notes (and the Obsidian connector, already cross-platform) |
| Reminders (`macctl.rs`) | **BYTE's own reminders as Windows toasts** (with Snooze / Done buttons); Outlook tasks through COM if classic Outlook is installed | BYTE's own reminders as desktop notifications; Evolution Data Server over D-Bus on GNOME |
| Calendar, EventKit/AppleScript (`macctl.rs`, `briefing.rs`) | Classic Outlook through COM; the calendar-link connector (ICS) works everywhere | Evolution Data Server (GNOME) / Akonadi (KDE) over D-Bus; ICS links everywhere |
| Mail drafts (`macctl.rs`) | Classic Outlook through COM (draft opened, never sent); else `mailto:` in the default app | Thunderbird `-compose`, else `xdg-email` |
| Messages (iMessage) | **Not possible** (no API). Offer: copy the text and open Phone Link | **Not possible**. Offer: copy the text |
| Music app control | **Any media app**: play / pause / next / "what's playing" through System Media Transport Controls (Spotify, browsers, VLC…) | **Any media app** through MPRIS over D-Bus |
| Safari's current tab (`macctl.rs`, web clipper) | The current tab of Edge/Chrome/Firefox through UI Automation; the bookmarklet clipper is already cross-platform | AT-SPI where it works; the bookmarklet clipper |
| Shortcuts app (`shortcut_make.rs`, automations) | BYTE's own automations + run **PowerShell scripts**; hand off to Power Automate Desktop if installed | BYTE's own automations + shell scripts |
| Settings: dark mode, volume, Do Not Disturb (`macctl.rs`) | **More:** dark mode (registry), volume and output device (Core Audio), Focus Assist, night light, brightness (WMI), power plan, Wi-Fi and Bluetooth on/off | **More:** dark mode (gsettings / KDE config), volume (PipeWire `wpctl`), DND, brightness (`brightnessctl`), Wi-Fi (`nmcli`), Bluetooth (`bluetoothctl`) |
| Files and Finder (`filectl.rs`) | File Explorer; delete to the **Recycle Bin**; the Windows Search index replaces Spotlight (`mdfind`) | Delete to the trash (freedesktop trash spec); `locate`/`plocate` or Tracker/Baloo for search |
| Terminal (`terminal.rs`) | **PowerShell** (and WSL commands if WSL is installed), same approval card | bash/zsh, same approval card |
| Storage cleanup (`upkeep.rs`) | Temp folders, the Recycle Bin, Downloads, old installers, browser caches; Windows Update cleanup through Disk Cleanup (needs admin: ask) | `~/.cache`, trash, old Flatpak runtimes, snap revisions, `apt clean` / `dnf clean`, journal vacuum (needs admin: ask through `pkexec`) |
| Battery health (`upkeep.rs`, `pmset`) | `powercfg /batteryreport` (laptops) | `/sys/class/power_supply` / UPower |
| "How's my computer doing" (`dashboard.rs`, `upkeep.rs`) | CPU, RAM, **GPU load, VRAM and temperature (NVML)**, disk, top apps | Same, with NVML / `sensors` |
| App uninstaller (`upkeep.rs`) | Installed apps from the registry; uninstall through `winget` or the app's uninstaller (approval) | apt / dnf / flatpak / snap remove (approval, `pkexec`) |
| Open at login (`lib.rs`) | Run key in the registry | `~/.config/autostart/*.desktop` |
| Scheduled tasks (`scheduler.rs`) | Same while BYTE runs; optionally **Task Scheduler** so a briefing runs even when BYTE is closed | Same; optionally a **systemd user timer** |
| iCloud Drive backups (`backup.rs`) | **OneDrive** folder if present, else any folder | Any folder (Nextcloud/Syncthing folders work) |
| Notifications | **Toasts with buttons and inline reply** ("Reply to BYTE…" right in the notification) | Desktop notifications with actions (freedesktop) |
| Deep links `byte://` (clipper) | Same (Tauri registers the scheme) | Same (`.desktop` MIME handler) |
| Signing and install | Unsigned NSIS installer ("More info → Run anyway"); updater signed with the same key | AppImage + `.deb` (+ maybe Flatpak); same updater key for AppImage |

### Extras Windows can do that the Mac can't

- **Game-aware BYTE:** when a full-screen game or a GPU-heavy app starts, BYTE frees the GPU: it unloads or moves
  the model to the CPU, and brings it back after. Opt in, ask once. Made for a gaming PC like Logan's.
- **"Why did my PC crash / freeze?"** Read the Windows Event Log and Reliability Monitor and explain in plain
  words (read-only).
- **Install and update apps with winget:** "update all my apps", "install VLC", each with an approval card.
- **Startup apps:** list what starts with Windows and turn items off (Task Manager's list), with Undo.
- **Drivers and GPU:** report the NVIDIA driver version, VRAM use and temperatures, and say when a newer driver is
  out (read-only; link to NVIDIA).
- **Toasts you can answer** without opening BYTE.
- **Much bigger models:** 16 GB of VRAM plus 32 GB RAM runs models the 16 GB Mac can't, and image generation fast
  (v1.1.0).

### Extras Linux can do that the Mac can't

- **System updates and packages** (apt/dnf/flatpak/snap) with approval through `pkexec`.
- **"What went wrong?"** Read `journalctl` (and `dmesg`) and explain failures in plain words (read-only).
- **Services:** list, start or stop systemd user services, with approval.
- **Ask about highlighted text** with no copying (the PRIMARY selection).
- **Any media player** through MPRIS, and deeper settings control through D-Bus.
- **Scheduled tasks while BYTE is closed** through systemd user timers.

### Idea to ask Logan about first (not in scope yet)

- **"BYTE Home":** the PC serves its models to his other devices on the home network, so the Mac can use the
  5080 like a private cloud. It would reuse the existing cloud-mode client. This needs his OK and a security
  design: pairing and an encrypted connection, never open to the internet.

## The work, in milestones

Report to Logan after each one: what works, screenshots, numbers, what's next.

### W1. Windows: BYTE builds, runs, and answers on the GPU (see "Any hardware" above: all vendors and memory modes)
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
