# BYTE for Android (planned)

Owner decision, 2026-10-04: **BYTE gets an Android app.** The owner's phone is a Galaxy Z Fold8 Ultra. Like the
Windows and Linux apps, it is its own native app with no linking to the Mac or PC (decision of 2026-10-02). iOS
stays out (see `DESIGN-AND-PLATFORMS.md`: free Apple signing expires every 7 days).

**When:** after 1.0 and the Windows app, before Linux (owner, 2026-10-04). This file holds the plan; nothing is built yet.

## The rule: models for phones up to 16 GB of RAM
- The desktop catalog goes up to 128 GB machines. The Android catalog covers **phones with up to 16 GB of RAM**,
  and it's **expanded with more models in that range**: more small and mid-size models, more quantizations of
  each, and MoE models with few active parameters. Those fit and run fast on phones.
- Android itself and other apps use several GB, and the system kills apps that take too much. So the fit planner
  (`system.rs`) works from what's actually free, with a safety margin. That's roughly 8–10 GB for model + context
  on a 16 GB phone; measure it on the Fold8 Ultra before fixing the numbers.
- The same per-device intelligence as the Mac:
  - detect the chip and GPU (Snapdragon / Adreno, Exynos / Xclipse, Tensor, Dimensity);
  - show estimated speed per model before downloading, then a measured speed after (`speed.rs`);
  - per-model tuning (`tune.rs`).
- Phones get hot. When the phone reports thermal throttling, BYTE notes it, and the Fast mode prefers smaller
  models.

## How it's built (reuse, don't rewrite)
- **Tauri 2's Android target** reuses the Rust core and the React UI. The layout adapts:
  - the folded cover screen gets a phone layout (one column, bottom composer);
  - unfolded, it gets the tablet layout (sidebar + chat, like the desktop).
- **Engine:** Android can't run a bundled executable the way the desktop sidecar does, so llama.cpp is linked into
  the app as a library (called directly from the Rust core through Rust bindings), with the same chat / streaming interface `chat.rs` uses
  today behind the `ModelBackend` boundary (`backend.rs`).
  - Backends: CPU (ARM i8mm/dotprod, KleidiAI), plus Vulkan or OpenCL on Adreno where faster. Pick per device by
    measuring, like the desktop memory modes.
- **Voice:** whisper.cpp and sherpa-onnx as libraries (both support Android), or Android's own speech and TTS as a
  lighter choice.
- **Storage:** the same encrypted SQLite (SQLCipher) in app storage; keys in the Android Keystore.

## Mac features → Android
| Mac | Android |
|---|---|
| Texting (Messages) and the Messages inbox | Real SMS: send with `SmsManager` after the same editable card; read the inbox with SMS permission; new-text banner with Draft a reply / Reply |
| Reminders, Calendar | `CalendarContract` (calendar), BYTE's own to-dos with Android notifications |
| Notes, files, knowledge base | Folders picked with the system file picker (Storage Access Framework) |
| Quick Ask (⌥Space), menu-bar icon | A home-screen widget, the share sheet ("Share to BYTE"), Quick Settings tile |
| "Hey BYTE" | While BYTE is open; always-on listening only as an opt-in foreground service (battery cost shown) |
| Touch ID lock | Fingerprint / face unlock (BiometricPrompt) |
| Mac upkeep, terminal, AppleScript control | Not on Android (phones don't allow it); storage view for BYTE's own models and files only |
| Web agent, research, documents, study, voices | Same as the desktop |

## Shipping without the Play Store
- A signed APK on GitHub Releases. The signing keystore is made by the owner (free) and kept as GitHub secrets,
  never in the repo.
- Updates: BYTE checks the same release feed and installs the new APK with Android's installer (the person taps
  Install). The Play Store ($25 once) is optional, later.
- `release.yml` gets an Android job (Android SDK + NDK on the Linux runner).

## Milestones
1. **A1:** builds and installs on the Fold; downloads a small model; answers on the phone's own hardware; shows
   estimated and measured speed.
2. **A2:**
   - the expanded ≤16 GB catalog;
   - fit planning from free memory;
   - per-device tuning;
   - the cover-screen and unfolded layouts.
3. **A3:**
   - web research, files, notes, documents and voice;
   - texting (SMS) and the inbox;
   - calendar, notifications, widget and share sheet.
4. **A4:** signed APK releases with in-app updates; checklist on the owner's phone.
