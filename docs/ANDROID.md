# BYTE for Android

The Android app is BYTE on a phone: the same chat, notes, documents and models, running on the phone's own processor, with BYTE Cloud
as an optional bigger brain. It is its own app, separate from the Mac and Windows apps (no syncing between them). It targets phones
with up to 16 GB of RAM, and the first phone it is tuned on is a Galaxy Z Fold.

**Status:** test builds (`.apk` on the [releases page](../../releases)). Not for daily use yet. Test builds are signed with BYTE's own key, so they
install over the previous one (the first one after an older build needs that older build uninstalled).

**Building:** `scripts/build-android.sh` builds the engines with the NDK and then the app; the `Android` workflow does the same on
every push to the `android` branch. The Rust core must keep compiling for `aarch64-linux-android`, so desktop-only code stays behind
`#[cfg(desktop)]`.

## The phone
- **the owner's test phone:** Samsung Galaxy Z Fold8 Ultra (a foldable: a cover screen and a large inner screen).
- Don't trust spec sheets; read the real values over adb:
  - `adb shell getprop ro.soc.model`, `ro.product.model`, `ro.build.version.release` (Android version);
  - `adb shell cat /proc/meminfo` (RAM and swap / RAM Plus);
  - `adb shell dumpsys gpu` or `vulkaninfo` (GPU, Vulkan);
  - `adb shell cat /proc/cpuinfo` (big and little cores; i8mm/dotprod flags).
- Record them in your first report, and in the chip table (below) so speed estimates use them.
- **Connecting it:** a USB cable to the PC, or **Wireless debugging** (Settings → Developer options; the owner turns on
  Developer options by tapping Build number 7 times). Then `adb pair` / `adb connect`. the owner approves the
  debugging prompt on the phone.

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
- **AI first: other apps go to storage, the AI keeps the RAM (owner, 2026-10-04).** While BYTE is in use, the
  model gets the real RAM, and the other open apps are moved out to RAM Plus. They stay open, just paused in
  storage, which is fine because they're less sensitive to speed than the model.
  - **How it works:** this is how Android's memory manager already behaves, and BYTE leans into it. The model's
    memory is used for every word, so the system keeps it in RAM, while apps sitting in the background are idle,
    so the system moves them to RAM Plus first.
  - **Counting the space:** BYTE's budget counts the memory those background apps can give up (measured by the
    per-phone memory test with RAM Plus on), not just what's free right now. So bigger models fit than the "free
    memory" number alone suggests.
  - **When AI focus is on (owner, 2026-10-04):** only while **BYTE is open on screen** or **Ask BYTE** (the
    assistant overlay, the side button or the text-selection menu) is in use. That includes an answer still being
    written after the person switches away, which is finished first.
  - **Loading on demand:** opening BYTE or Ask BYTE starts loading the model right away, before the person has
    finished typing or speaking. The weights are memory-mapped, so a reopen within minutes is near-instant because
    they're often still cached.
  - **Leaving:** when BYTE leaves the screen and nothing is being answered, it waits a short grace period (default
    2 minutes, adjustable, so a quick app switch doesn't reload), then unloads the model. The RAM goes back to the
    other apps.
  - **Switching it off:** the person can turn AI focus off. Then BYTE takes only what's free, and other apps stay
    in RAM.
  - **The trade-off, said plainly in Settings:** switching back to another app can take a moment while it comes
    back from storage, and a few apps may reload. Nothing is lost.
  - **When BYTE closes or the model unloads:** the RAM goes back to everything else right away.
  - **Not done:** BYTE never closes other apps itself, and never pins memory in ways Android forbids
    (`mlock` is capped for apps).
  - Keep RAM Plus on for this; the onboarding explains it and suggests the largest RAM Plus size if the person has
    the storage.
- The same per-device intelligence as the Mac:
  - detect the chip and GPU (Snapdragon / Adreno, Exynos / Xclipse, Tensor, Dimensity);
  - show estimated speed per model before downloading, then a measured speed after (`speed.rs`);
  - per-model tuning (`tune.rs`).
- Phones get hot. When the phone reports thermal throttling, BYTE notes it, and the Fast mode prefers smaller
  models.

## How it's built (reuse, don't rewrite)

### The app
- **Tauri 2's Android target** reuses the Rust core and the React UI.
  - `src-tauri/src/lib.rs` already has `#[cfg_attr(mobile, tauri::mobile_entry_point)]`, and `Cargo.toml` already
    builds a `cdylib`.
  - `npm run tauri android init` creates `src-tauri/gen/android` (commit it).
  - Run on the phone with `npm run tauri android dev`; build with `npm run tauri android build --apk`.
- **The layout adapts.** The folded cover screen gets a phone layout: one column, the composer at the bottom, the
  sidebar as a drawer. Unfolded, it gets the tablet layout (sidebar + chat, like the desktop).
  - Detect this from the window width and the Fold posture, not the device name.
  - Touch targets are at least 48 dp. The keyboard must not cover the composer.
- **Desktop-only plugins and code.** These don't exist on Android, so move them under `cfg(desktop)` in
  `Cargo.toml` (`[target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies]`) and in `lib.rs`:
  `tauri-plugin-global-shortcut`, `-autostart`, `-updater`, the tray, and the Quick Ask window (`quick.rs`).
  - Android gets its own versions: the default assistant and Ask BYTE (Quick Ask), the in-app updater (updates),
    and a widget or Quick Settings tile (the tray icon).
- **Android APIs** (SMS, notifications, Accessibility, assistant role, camera, ML Kit, location, BiometricPrompt,
  Keystore) live in a **Tauri mobile plugin written in Kotlin** (`src-tauri/plugins/byte-android/`). Rust calls it
  through `PluginHandle::run_mobile_plugin`. Permissions go in its `AndroidManifest.xml`, asked at first use with an
  explanation.

### The engine (biggest reuse: keep `llama-server`)
- Desktop BYTE runs llama.cpp's `llama-server` as a sidecar and talks to it over local HTTP (`engine.rs`,
  `chat.rs`, `speed.rs`, `tune.rs`, `embed.rs`). **Keep that on Android**, so every chat, tool, research and tuning
  path works unchanged:
  - **Building:** cross-compile `llama-server` with the NDK (r27+, `arm64-v8a`, `-DGGML_OPENMP=OFF`, plus
    `-DGGML_VULKAN=ON` and/or `-DGGML_OPENCL=ON` for Adreno). Use the tag pinned in `scripts/LLAMA_TAG`, in a new
    `scripts/build-llama-android.sh`.
  - **Packaging:** ship it as `jniLibs/arm64-v8a/libllama-server.so` (an executable named like a library), with
    `android:extractNativeLibs="true"`. Android (10+) only lets apps run executables from their **native library
    folder**, so it's started from `getApplicationInfo().nativeLibraryDir`. Running from app data is blocked
    (W^X).
  - **Starting it:** `tauri-plugin-shell`'s sidecar API doesn't run on mobile, so `engine.rs` starts it with
    `std::process::Command` from that folder under `cfg(target_os = "android")`. The port, API key, health check,
    warm-up and restart logic stay the same.
  - **The same trick for the other tools:** `whisper-cli`, `sherpa-diarize` and `sherpa-tts`
    (`build-whisper.sh` / `build-sherpa.sh` get Android variants).
  - **Fallback:** if a future Android version blocks it, link llama.cpp as a library behind the same `ModelBackend`
    (`backend.rs`) and keep the HTTP-shaped interface in Rust. Only do this if the executable route fails; note it
    in the report.
- **Backends:** CPU (ARM i8mm/dotprod, KleidiAI), Vulkan and OpenCL (Adreno). `tune.rs` measures them per phone and
  picks the fastest, like the desktop memory modes.
- **Threads:** the big cores only (the count is read from `/proc/cpuinfo` / cpufreq), never the little ones for
  generation.

### The rest
- **Voice:** whisper and sherpa as above, or Android's own speech recognition and TTS as a lighter choice. `cpal`
  supports Android (AAudio), for "Hey BYTE" and the speaking pipeline (`tts.rs`).
- **Storage:** the same encrypted SQLite (SQLCipher: `rusqlite` `bundled-sqlcipher-vendored-openssl` builds with
  the NDK) in app storage.
  - Secrets (the BYTE Cloud key, connector tokens) go in the **Android Keystore**: `cloud/keychain.rs` gets an
    Android `SecretStore`.
  - Models live in the app's external files folder (`getExternalFilesDir`) so they don't fill internal app data,
    and are shared by profiles like on the Mac.
- **Hardware info (`chip.rs`, `system.rs`):** add an Android chip table that maps the SoC (from `ro.soc.model`) to
  memory bandwidth, big-core count and GPU, for speed estimates before download. `speed.rs` then measures the real
  speed. RAM comes from `/proc/meminfo` and `ActivityManager.MemoryInfo`; thermal state from `PowerManager`
  (`getCurrentThermalStatus`, `getThermalHeadroom`).
- **Catalog:** the same `src-tauri/catalog/models.json`, built by `scripts/discover-models.mjs` →
  `build-catalog.mjs` → `enrich-catalog.mjs`.
  - Add a phone pass that adds more small and mid-size models, more quantizations (Q4_0 / Q4_K_M / IQ4 and other
    ARM-friendly ones), and small-active MoE models.
  - Filter by what fits the phone's measured budget, so the Android catalog shows models for phones up to 16 GB.
  - `models::recommend` already scores by fit, quality and speed: feed it the phone's numbers.
- **Mac-only code** (`#[cfg(target_os = "macos")]`) already has fallbacks for other systems. The list of files is
  in `PORTING-WINDOWS-LINUX.md` ("What exists, and what's Mac-only"). On Android, each feature either gets an
  Android version (the feature map below) or a clear "not on Android" message, never a crash.

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

## Extras only Android can do 
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

## Setting up (on the PC; Windows or Linux both work)
- Android Studio (or the command-line tools): SDK Platform 35+, Build-Tools, Platform-Tools (adb), **NDK r27+**,
  and **JDK 17**.
- Set `ANDROID_HOME` and `NDK_HOME`.
- `rustup target add aarch64-linux-android` (add `armv7-linux-androideabi`, `x86_64-linux-android` and
  `i686-linux-android` only if needed; the Fold is arm64).
- Node 22, then `npm ci` and `npm run tauri android init`.
- Tauri's Android prerequisites page has the exact steps. Follow it rather than memory.

## Shipping without the Play Store
- **A signed APK on GitHub Releases** (`BYTE_<version>_android-arm64.apk`) next to the Mac files, in the same
  release, with the same version (`node scripts/bump.mjs` also sets the Android `versionName` / `versionCode`;
  extend it).
- **Signing keystore:** the owner makes it once on his PC with `keytool -genkeypair -v -keystore byte-release.jks
  -keyalg RSA -keysize 4096 -validity 10000 -alias byte`. Keep the file safe and backed up: losing it means users
  can't update.
  - He adds four GitHub secrets himself: `ANDROID_KEYSTORE_BASE64` (`base64 -w0 byte-release.jks`),
    `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.
  - Never ask for them in chat. Don't commit the `.jks`; add `*.jks` / `*.keystore` to `.gitignore` and to
    `scripts/check-secrets.sh`.
- **Updates:** Tauri's updater plugin is desktop-only, so Android gets a small in-app updater.
  - It reads `android.json` from the latest release (`{version, url, sha256, notes}`, written by `release.yml`),
    downloads the APK, checks the SHA-256, and opens Android's installer (`REQUEST_INSTALL_PACKAGES`; the person
    taps Install).
  - Android itself refuses an update signed with a different key.
  - Show it in Settings → About like the Mac's update row (`components/update/UpdateRow.tsx`, `lib/updateNotes.ts`).
- **`release.yml`:** an `android` job on `ubuntu-latest`:
  - set up the JDK, SDK and NDK; build the sidecars with the NDK (cache them);
  - `npm run tauri android build --apk`, signed from the secrets;
  - upload the APK and `android.json`.
  - With no keystore secret it builds an unsigned debug APK and warns, like `scripts/updater-key.sh` does for the
    Mac.
- **Never push to `main` while a release run is in progress.** Dispatch `release.yml` only
  for the branch head.
- The Play Store ($25 once) is optional, later. Accessibility and notification replies need extra review there.

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
5. **A5:** signed APK releases with in-app updates; a `docs/CHECKLIST-ANDROID.md` (like `CHECKLIST-1.0.md`) that
   the owner runs on the Fold.

Start with **A1**: `tauri android init`, the desktop-only plugins behind `cfg(desktop)`, `llama-server` built with
the NDK and started from the native library folder, then a small model (Qwen3 0.6B) answering on the Fold, with
its tokens per second.
