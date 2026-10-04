# BYTE for Android (planned)

Owner decision, 2026-10-04: **BYTE gets an Android app.** The owner's phone is a Galaxy Z Fold8 Ultra. Like the
Windows and Linux apps, it is its own native app with no linking to the Mac or PC (decision of 2026-10-02). iOS
stays out (see `DESIGN-AND-PLATFORMS.md`: free Apple signing expires every 7 days).

**When:** after 1.0 and the Windows app, before Linux (owner, 2026-10-04). This file holds the plan; nothing is built yet.

## The rule: models for phones up to 16 GB of RAM
- The desktop catalog goes up to 128 GB machines. The Android catalog covers **phones with up to 16 GB of RAM**,
  and it's **expanded with more models in that range**: more small and mid-size models, more quantizations of
  each, and MoE models with few active parameters. Those fit and run fast on phones.
- **Use as much RAM as the phone can safely give (owner, 2026-10-04: "max use and speed").** No fixed cap. BYTE
  takes everything that's free without getting the app killed or making the phone stutter:
  - **Reading the budget:** it comes from the phone's live numbers (`ActivityManager.MemoryInfo`: `availMem`,
    `threshold`, `lowMemory`, plus `/proc/meminfo`), not a guess. Model, context and cache grow to fill it, minus a
    small margin for Android.
  - **Backing off:** when Android warns of memory pressure (`onTrimMemory`, low-memory signals), BYTE shrinks
    first: it trims the context cache, then unloads helper models, and never lets the system kill it mid-answer.
    When memory frees up again, it grows back.
  - **While answering:** a foreground service keeps BYTE alive and at full speed.
  - **Measuring the real ceiling:** a one-time memory test on each phone (load in steps, watch for pressure)
    finds what that phone can really hold, and it's saved like `tune.rs`'s per-Mac tuning. On a 16 GB Fold8 Ultra
    that's probably 10–12 GB for the model, but the test decides.
  - **Speed:** model weights are memory-mapped (no second copy), the CPU threads are pinned to the big cores, and
    the GPU (Vulkan/OpenCL) is used where measuring shows it's faster.
- **Storage used as RAM (Samsung "RAM Plus", virtual memory):** the phone sets aside storage as extra memory. It
  helps keep apps open, but storage is many times slower than real RAM, and a model reads all its weights for every
  word. So:
  - BYTE detects RAM Plus and shows it, but **doesn't count it as RAM** when deciding what fits. Counting it would
    make answers crawl.
  - **Stretch option:** the person can choose to run a model a bit bigger than RAM, with part of it read from
    storage (like the desktop's "partly on CPU" stretch mode). It's labelled "slower" with the measured speed, and
    is never picked automatically. It's best for MoE models, which only read a small part of their weights per word.
  - Since phones with RAM Plus reserve that storage, the onboarding mentions turning it down if the person wants
    more free storage for models.
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

## Extras only Android can do (owner: "add all of it", 2026-10-04)
Each is a module the person can turn off, and anything that acts still shows the approval card first.

**Everywhere on the phone**
- **Default assistant:** BYTE can be chosen as the phone's digital assistant (`RoleManager.ROLE_ASSISTANT` /
  `VoiceInteractionService`), opened by a long press of the side button or a corner swipe instead of Gemini or Bixby.
- **"Ask BYTE" on selected text in any app:** an entry in the text-selection menu (`ACTION_PROCESS_TEXT`) to reply,
  translate, explain or rewrite, like ⌥⌘B on the Mac. It can put the result back in place.
- **Reply in any messaging app:** with notification access (`NotificationListenerService`), BYTE sees messages from
  WhatsApp, Messenger, Discord, Instagram and so on. It drafts a reply on the editable card and sends it through
  the notification's own reply action (`RemoteInput`).
  - **"What did I miss?"** summarizes recent notifications.
  - Notifications are read on the phone only, and are never stored beyond the summary the person asks for.
- **"What's on my screen?":** with Accessibility permission (`AccessibilityService`), BYTE reads the current screen
  to explain, summarize or help fill it in. Any tap or typing it does is shown on an approval card first. It is off
  by default, with a clear note on what it allows.

**Camera**
- **Point and ask:** a live camera view; take a photo, then ask. Covers plants, broken parts, homework, labels and
  recipes. It uses a vision model when one fits, and otherwise the photo helper (`looker.rs`) or the text read from
  it.
- **Live translate of signs and menus:** on-device text recognition (ML Kit, offline) plus BYTE's translate, drawn
  over the camera view.
- **Document scanner:** edge detection and cleanup (ML Kit document scanner) produce a PDF or note, which goes into
  the knowledge base.

**Made for the Fold**
- **Interpreter mode:** a two-person conversation translated live.
  - Unfolded, the person sees their side on the inner screen while the other person reads and speaks on the cover
    screen (Jetpack WindowManager / rear display).
  - Speech uses whisper, then translate, then a voice.
- **Flex mode (half-folded):** the answer or camera on the top half, and the composer, keyboard or controls on the
  bottom (`FoldingFeature` posture).
- **Split screen and drag and drop:** BYTE works side by side with other apps, and text, photos and files can be
  dragged in and out.

**Your day**
- **Location reminders:** "remind me when I get home / leave work" (geofences, with location only when allowed).
- **Phone actions by voice or text:** alarms and timers (`AlarmClock` intents), Do Not Disturb (with permission),
  flashlight, opening apps and settings pages, and starting navigation.
- **Call screening:** labels or answers unknown callers (`CallScreeningService`). It doesn't record calls (Android
  blocks that).
- **Hands-free with earbuds:** pressing the headset button opens a voice turn (`MediaSession`).
- **No-signal mode:** answers, translation, notes and documents keep working with no internet. The offline switch
  turns on by itself when there's no connection, with a note saying so.

**Honest limits (shown in the app)**
- **Heat and battery:** long answers on big models warm the phone. BYTE shows the battery cost, and when the phone
  reports throttling it uses smaller models.
- Always-on "Hey BYTE" is opt-in for battery reasons.
- Screen reading and replying through notifications are harder to get into the Play Store. They're fine in the
  GitHub APK, so they ship there first.

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
4. **A4:** the extras:
   - default assistant, Ask BYTE on selected text, replies in any messaging app, what's on my screen;
   - camera: point and ask, live translate, scanner;
   - Fold: interpreter mode, Flex mode, split screen;
   - location reminders, phone actions, call screening, earbuds, no-signal mode.
5. **A5:** signed APK releases with in-app updates; checklist on the owner's phone.
