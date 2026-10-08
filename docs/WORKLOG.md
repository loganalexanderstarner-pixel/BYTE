# BYTE work log

Every change, newest first: what it did, why, where it lives, and how to undo it.
**Add an entry with every commit** (see CLAUDE.md). If something breaks, find the
change here and revert just that commit (`git revert <hash>`) instead of starting
over from an old copy of the repo.

**CI is free and runs on every push** -- `.github/workflows/ci.yml` says so in its
own header, because Actions minutes are unlimited for public repositories. This
paragraph previously said minutes were limited and to run CI by hand only at
milestones; that was stale and actively harmful, since it told sessions to skip
the one verification that needs no local toolchain. Corrected 2026-10-04 after
CI caught several problems a local run had missed.

Still run `scripts/check-all.sh` before pushing (secret scan, typecheck,
frontend tests + build, Rust tests, clippy) -- it is faster than waiting for CI
and catches the same things. The Mac engine test and test builds remain manual,
being slow rather than expensive.

Format: `hash — title` · **Why** · **What** (files) · **Verify** · **Undo**.

---

## 2026-10-04 (cluster session, Windows port: `claude/windows-port`)

These eight commits were pushed without worklog entries, against CLAUDE.md's own
rule. Recorded together here rather than pretended into separate entries.

### W1: the Windows app builds, runs, and the engine sees the GPU
- **Why:** first milestone of `docs/PORTING-WINDOWS-LINUX.md`.
- **What:** toolchain on the PC (MSVC 14.44, CUDA 13.4, Vulkan SDK 1.4.363, Rust
  1.99, Node 22 pinned to match CI, Strawberry Perl); `llama-server` built with
  CUDA for Blackwell and separately with Vulkan, both installed as sidecars;
  Rust core compiles; frontend builds; `byte.exe` runs.
- **Verify:** `--list-devices` reports the discrete GPU; CI green on all jobs.

### Four findings worth keeping
- **A GUI app cannot start from an SSH session.** WebView2 fails with "Invalid
  window handle" because an SSH session is session 0 with no desktop. Launch via
  Task Scheduler with `/IT` to reach the interactive session. Cost two rounds of
  debugging a non-bug.
- **`rusqlite`'s `bundled-sqlcipher-vendored-openssl` needs perl on Windows.**
  CI never caught it because GitHub's windows runner ships Strawberry Perl, so
  the documented setup was incomplete while CI stayed green. Added to the brief.
- **The VC++ runtime DLLs the engine needs do not ship with Windows.** On the
  test machine they were created twelve days *after* the OS install, by a
  redistributable. So the installer must carry `MSVCP140`, `VCRUNTIME140`,
  `VCRUNTIME140_1` and `VCOMP140`, or the engine fails to load on a clean
  machine with a cryptic error. Found without needing a clean VM.
- **CUDA and Vulkan differ where it matters.** *(Corrected 2026-10-05: the 39x below was wrong; Vulkan is about 12% slower reading prompts. See "A 39x number was wrong" further down.)* Same card, same model, same
  llama.cpp: generation 204.8 vs 192.1 tok/s (1.07x), prompt processing 9,827 vs
  251 tok/s (**39x**). A 10,000-token document is about a second on CUDA and
  about forty on Vulkan. This is why the installer bundles CUDA.

### Three planner defects, found by fixtures and fixed
`src-tauri/src/hardware_fixtures_tests.rs` tests machines nobody owns by feeding
hardware descriptions to the planner, which is pure arithmetic. It found:
- **`gpu_budget()` returned a share of system RAM** -- correct for unified
  memory, structurally wrong for a discrete card. On a 32 GB PC it offered
  21.3 GB to a 16 GB card, so BYTE would have recommended a model that cannot
  load. Fixed by `gpu_budget_for(total_ram, vram, override)`; the Apple path is
  unchanged and a test asserts that.
- **`identify()` read PC CPU names as Apple chips.** "Intel Core Ultra" matched
  the Ultra tier and "Ryzen PRO" the Pro tier, each then given an M-series
  chip's bandwidth and TFLOPS. Parsing is now gated on the brand being Apple.
- **Speed estimates ignored the backend and over-promised weak hardware.** Now
  calibrated per backend from the measurements above, and the unknown-hardware
  default drops from 50 to 24 GB/s after a 4B model on four Cortex-A76 cores
  measured 3.26 tok/s where the old default predicted 16.7.
- **Verify:** `cargo test hardware_fixtures -- --include-ignored`; three fixtures
  remain ignored, covering PC bandwidth data and Apple-shaped RAM advice.
- **Undo:** revert `a4d5028`, `3a41380`, `25d8ed1`, `56eec17` individually.

### 2026-10-05: packaging, the first ported call sites, and four mistakes worth keeping
- **The Windows bundle produced nothing and said it succeeded.** `bundle.targets`
  was `["app", "dmg"]`, both macOS-only. A release build on the PC compiled for
  12 minutes, exited 0 and wrote no installer. Now NSIS, per user (no admin).
  Lesson: check for the deliverable, not the exit code.
- **`windows-latest` is not stable.** It became `windows-2025-vs2026`, which has
  only Visual Studio 2026, so a hard-coded `Visual Studio 17 2022` generator
  failed outright. The Windows jobs now pin `windows-2022` and the build scripts
  no longer pass `-G`.
- **No Rust test has ever run on Windows.** The test binary imports
  `TaskDialogIndirect` (Common Controls v6) and has no manifest, so it dies at
  load with `0xC0000139`. `byte.exe` has the manifest; the test exe does not
  (checked on a real PC). CI only runs `cargo check` on Windows, so nobody saw.
  Delay-loading `comctl32` fixes it, but only via `RUSTFLAGS` for test runs: the
  `build.rs` route is closed because `rustc-link-arg-tests` is not an instruction
  Cargo accepts, and the general `rustc-link-arg` would change the shipped app.
- **A build-script change I pushed without testing broke the Windows build** for
  roughly two hours (`4f20a9e`, reverted in `94d55c0`). Anything that affects the
  build gets tried in a throwaway crate first.
- **Ported so far (3 of 37 Apple call sites):** OCR via `Windows.Media.Ocr` and
  `Windows.Data.Pdf`; the app lock via Windows Hello; the clipboard via Win32.
  Known limits: HEIC photos need Microsoft's HEIF extension; Windows Hello needs a
  PIN, fingerprint or face, where macOS accepts a password.
- **Verified at run time (2026-10-05, on the PC):** OCR renders a PDF page and
  reads "48213" back; the clipboard round-trips accented, Chinese and emoji text
  through UTF-16 (a real round trip, not a skip); Windows Hello reports available.
  The full Rust suite is **514 passed, 0 failed, 57 ignored**, the first it has
  ever run on Windows. Still needs a person: the Hello prompt itself, and OCR on a
  real photo.

### 2026-10-05 (later): the two failures, and what fixed them
- **CUDA headers (Windows engine test).** The failure was `crt/host_config.h` not
  found. NVIDIA's own component manifest for 13.3.1
  (`developer.download.nvidia.com/compute/cuda/redist/redistrib_13.3.1.json`)
  shows CUDA 13 split the crt headers out of `nvcc` into a `cuda_crt` component,
  and replaced `thrust` with `cccl`. The header was confirmed inside the archive.
  The install list in `.github/actions/windows-engines/action.yml` now has `crt`
  and `cccl`. **Read the manifest, not the README** (it lists no package names).
  Whether the installer accepts those two names is decided by the next run.
- **`scripts/build-android.sh` fixed on `claude/android-port` (`f51a43d`).** It
  needed, and now sets up itself: `AR_`/`RANLIB_aarch64_linux_android` pointing at
  the NDK's `llvm-ar`/`llvm-ranlib` (vendored OpenSSL otherwise fails with
  `aarch64-linux-android-ranlib: not found`), empty sidecar placeholders, and a
  check for SDK platform 36. On failure it used to print nothing, because it only
  printed what a grep matched and the grep matched nothing; it now prints the log
  tail too. **Proven:** with placeholders and the old APK deleted and only
  `ANDROID_HOME`, `NDK_HOME`, `JAVA_HOME` set, it exits 0 and builds a fresh
  149 MB debug APK. The Android workflow failed at this step before the fix.
- **Android pushes publish a public pre-release.** `android.yml` runs on every
  push to `claude/android-port` and creates an `android-test-N` pre-release with
  the debug APK (never "latest", so the Mac update check is unaffected). It is a
  deliberate design of the Android session, and it means the APK can be installed
  by opening the Releases page on the phone, with no adb.
- **Voice on Windows:** `cpal` was declared only under macOS, so no audio backend
  was compiled on Windows at all; widening the `cfg` checks alone would have done
  nothing. Now declared for macOS and Windows. Not yet exercised at run time.
- **Voice verified end to end on the PC (2026-10-05):** WASAPI sees the default
  output and input (48 kHz); Windows' own `tar.exe` unpacks a real `.tar.bz2` voice
  package; `sherpa-tts` writes a WAV; `whisper-cli` transcribes it back ("Hello,
  this is Bite Speaking on Windows" for "...BYTE...": a normal mishearing by the
  tiny model, not an engine fault). `voices.rs` now names `System32\tar.exe` rather
  than leaving it to PATH, where a Git GNU `tar` would read `C:\...` as a remote
  host. Still needs a person: that WebView2 asks for and keeps microphone
  permission, that the wake chime is audible, and spoken output reaching the
  headset through the app itself.
- **Trap I fell into:** piping a build to `tail` hides its exit code. Redirect to a
  file and print `$?` instead.

### 2026-10-05 (evening): the PC's first screens, looked at on a real PC
A driver on the PC (a script run through Task Scheduler's interactive session)
clicks through the real app and saves a screenshot of only BYTE's window. That is
how every item below was found; none of them showed in a test.
- **Window override (`f2189a4`, `77661d1`).** `tauri.windows.conf.json` had an
  `app.windows` array. Tauri merges configs as a JSON merge patch, and a patch
  *replaces* arrays, so the whole window definition (title, size, minimum size)
  was lost and the app opened as an untitled default window. Now the file holds
  only `bundle`; the rule is in `docs/WINDOWS-PACKAGING.md`.
- **Wording (`4071312`, `c98b16d`).** 143 Mac-only strings go through `osText()`
  (identity on a Mac). A regex pass corrupted JSX text containing apostrophes, so
  it was redone as a TypeScript-AST codemod, and `scripts/audit-ui-text.cjs`
  finds any string that still reads wrong for the machine.
- **Features (`171616e`).** Windows no longer offers what only a Mac does (lock
  with Touch ID, Mac control, Messages inbox, ...) and defaults the shortcuts to
  Ctrl/Alt.
- **Model screen: "Best for this PC" was not BYTE's pick, and its speed was a
  fiction (this commit).** Checked on a real 16 GB card with 31 GB of RAM:
  - Onboarding sorted by quality alone, so the first card (a 23 GB model that
    mostly runs on the CPU) was not what *Continue* downloaded (`recommend()`'s
    pick, an 11 GB model that fits the card and runs at about 50 tokens/s). That pick was not even among the four
    cards shown. On a PC the list now puts the pick first and models that spill
    onto the CPU after those that stay on the card (`runnable` in
    `Onboarding.tsx`). A Mac's order is unchanged on purpose; only a 64 GB Mac
    would differ (its first card is not `recommend()`'s pick), left alone for now.
  - `offload_slowdown` was written for one pool of memory (a Mac), where expert
    layers on the CPU cost ~40%. Next to a discrete card the CPU reads ordinary
    RAM, so it now adds the two reads: the spilled model's estimate falls from
    about 73 to about 19 tokens/s. `PC_SYSTEM_MEMORY_GBPS = 40` is an assumption
    (dual-channel DDR5 measured 50-55 on the cluster's own PC, DDR4 is ~25);
    tuning replaces it with a measurement. `chip::achievable_gbps` is the old
    `estimate_on` efficiency arithmetic, moved out unchanged so both use one source.
  - Onboarding fetches the pick itself: the store only has it after the engine starts, so the
    first version of this fix silently did nothing (seen in the screenshot, not in a test).
  - Verify: on the PC the first card is the pick, selected, and the next three all stay on the
    card. Undo: `git revert` this commit; Mac behaviour does not depend on it.
  - Tests: `models::tests::spilling_onto_the_cpu_is_slow_next_to_a_discrete_card`,
    `a_pc_is_recommended_a_model_that_fits_its_card`, and four `runnable` cases.
- **More wording the screenshots found (Engine, Privacy, Automations).** The Engine tab said "llama.cpp with
  Apple Metal" on a PC (`engineWith()` in `platform.ts` now names CUDA, Vulkan or the processor from the
  backend), Privacy showed `⌘K` and a "paid Apple developer signature" sentence, Settings said "Quit with
  ⌘Q", reduced motion pointed at macOS's Accessibility → Display, and Automations offered Shortcuts/Siri.
  All reworded or hidden on Windows; the Mac text is identical (checked with `audit-ui-text.cjs`, whose only
  differences are strings now split across JSX).
- **Secrets could not be stored on Windows at all.** The UI said "Windows Credential Manager", but both secret
  stores (`cloud/keychain.rs` for the cloud key, `connectors/mod.rs` for Notion and calendar links) were
  stubs that returned an error off a Mac, and Connectors showed "need a PC for now" to a PC user. Both now use
  the `keyring` crate's `windows-native` backend. Limit to know: Credential Manager holds about 1,280
  characters per entry, so a very long list of calendar links would be refused with an error rather than
  saved. Test (ignored, writes to the real store): `cloud::keychain::windows_tests::credential_manager_round_trips`;
  it passes in the desktop session and fails over SSH with `ERROR_NO_SUCH_LOGON_SESSION` (a network logon has
  no credential store), so run it through `schtasks /IT` like the app.
- **Three more Mac-only gates opened for Windows.** (1) Deleting a note now moves it to the Recycle Bin
  (`recycle.rs`, `SHFileOperationW` with undo), like the Mac's Trash; before it erased the file for good.
  Checked in the real bin, and the test leftover removed. (2) The knowledge base reads text in photos on
  Windows too (`kb.rs`: it was gated to Mac although `ocr.rs` has done Windows OCR since earlier). (3) The
  "what is using memory" advice, blank on Windows because it only recognised Mac `.app` bundles, now lists
  programs that have a window on the desktop, named from their file description ("Google Chrome") as Task
  Manager does, all of an app's processes summed; Windows itself, Explorer, BYTE and its web view are never
  listed. Closing one posts `WM_CLOSE` to its windows, the same as the X button, so it can still ask to
  save. Verified against the real desktop (it found the one big app and named it correctly). **Not
  verified live:** the close itself, because it would close a real app; it is a single `PostMessageW`.
- **Opening Help blanked the whole app on Windows.** `useEffect(() => body.current?.scrollTo(0, 0), [id])`
  hands React whatever `scrollTo` returns as the effect's cleanup; on Windows' web view that is not
  `undefined`, so React threw "destroy is not a function" when the panel changed and the window went empty.
  Found only by clicking through the real app (an on-screen error handler showed the message). The effect is
  braced, so are the three other effects written as bare calls, and `src/lib/effects.test.ts` fails if any
  `useEffect(() => expression)` comes back. A cleanup returned on purpose (`() => () => flush()`) is fine.
- **Help in the words of the machine.** Articles had Mac steps throughout (Open Anyway, ⌘K, Touch ID, iCloud,
  System Settings). They now carry `<!-- mac -->` / `<!-- win -->` / `<!-- all -->` blocks where the steps
  differ (SmartScreen's "Run anyway", the microphone setting, Ctrl+Alt+B, Credential Manager) and are
  reworded mechanically elsewhere (`src/lib/helpText.ts`; the same rules in `src-tauri/src/platform_text.rs`
  for the copy the model quotes). The Mac text of every article is byte-for-byte what it was (checked
  against the old files). A test fails if a Windows article mentions ⌘, macOS, Finder, iCloud, Keychain,
  Touch ID, Open Anyway, System Settings or the menu bar. Retrieval ignores platform names ("Windows"
  appeared in every article and drowned the real match) and is tested to hand the model the fix on both.
- **The whole journey, in the real app, on the real GPU.** Real CUDA engine beside the debug app, a 0.6B model
  in the models folder, `activeModel` in settings; the driver (now with a `type:` step) waited for the engine,
  typed a message and pressed Enter. The engine started by itself (pill: "Qwen3 0.6B · 16k"), the reply
  streamed at about 510 tokens/s, the chat got a title, and it was still there after restarting the app.
  Also `chat::e2e_streams_from_real_llama_server` and `agent::e2e_agent_uses_calculator_tool` pass on the PC
  (thinking on and off, a tool call, 1234 × 5678 = 7,006,652).
- **Rust-side sentences said "Mac" on a PC** ("Your Mac can run Qwen3.8 27B…", "can't run on this Mac",
  "Your Mac ran out of memory", "BYTE is offline, so this answer was written on this Mac"). `platform_text::here()`
  rewords the fixed part of a message on Windows and is the identity on a Mac; model names are kept out of it.
- **ARM64 backend rule.** The Windows ARM64 build will carry the CPU engine only (CUDA and Vulkan are x64), so
  `gpu::effective_backend()` answers CPU there whatever graphics it finds; otherwise a Snapdragon's integrated
  GPU would have made the speed estimates assume a GPU that is not used. The build, packaging and CI for
  ARM64 are next, after the x64 installer run (they share the scripts it watches).
- **Test data shares the real app's folder.** The dev app and the installed app both use
  `%APPDATA%\com.loganstarner.byte`, so a test model, `settings.json` and chat database left there would
  appear in the person's real BYTE. Clean them after every driven test (settings, `byte.db*`, `db.key`, models).
- **A 39x number was wrong, and AMD graphics could be tested after all.** The docs, `chip.rs`, a test and
  the CUDA-bundling argument all rested on "Vulkan reads prompts 39 times slower than CUDA" (251 against
  9,827 tok/s). Re-measured on 2026-10-05, same card, same Qwen3 4B Q4_K_M, a 2,781-token prompt, prompt
  cache off, two rounds: CUDA 12,600 tok/s, Vulkan 11,090 (12% slower reading), generation 213 against
  194 (9%). The old figure was never repeated and cannot be explained; my first short-prompt run on the
  same day gave a similarly low Vulkan number, so a short or cached prompt is the likely culprit.
  Corrected in `chip.rs` (Vulkan prompt scale 0.026 -> 0.88, ceiling 400 -> 11,000), the fixture tests
  (now measured values, and a test that fails if Vulkan is predicted an order of magnitude slower),
  `gpu.rs`, and `docs/WINDOWS-PACKAGING.md`, which now says what CUDA actually buys (about 12%, for
  470 MB) and leaves the packaging choice (CUDA + Vulkan, or Vulkan + a small CPU build) to the owner.
  **I had also said AMD graphics could not be tested; the owner pointed out the test PC has an AMD
  integrated GPU.** It was disabled in Device Manager (problem code 22), not missing a driver. Enabled
  briefly (and disabled again afterwards): the AMD Windows driver provides Vulkan, the engine ran a
  model on it correctly (0.6B: 19-27 tok/s generating, 232 tok/s reading a 2,781-token prompt), and
  BYTE's detection lists both cards and picks the NVIDIA one. That measurement is now a table entry
  (`AMD RADEON(TM) GRAPHICS` = 20 GB/s, 0.35 TFLOPS; the generic guess for integrated graphics promised
  about twice the generation speed and twelve times the prompt speed of this chip) with a test.
  Still untestable here: Intel graphics, NPUs, an AMD *discrete* card on the Windows driver (the other
  node's RX 7900 GRE runs Linux, which tests the Vulkan engine on AMD but not the Windows driver).
- **The terminal on Windows (PowerShell), same flow as the Mac, stricter policy.** Owner said yes, "safely like
  it is already". `terminal_ps.rs` is the policy; `terminal.rs` runs the same card for both shells (`Shell`).
  The Mac side keeps a refusal list; PowerShell can reach far more things by far more indirect routes
  (aliases, `iex`, .NET calls, encoded commands), so on Windows a model-written command runs only when
  **every** command in it is on a list of things that read or show (plus careful single-file changes: no
  wildcards, no `-Recurse`/`-Force`, nothing in Windows' or app folders or the registry, never from a pipe,
  one change per command, no critical process names), and then only after **Do it**. Also refused: backticks,
  `$()`, `&`, redirects, `#`, non-ASCII outside quotes, `-EncodedCommand`/`-MemberName`, `[type]::` calls
  outside `[math]`/dates/strings, and method calls outside a list of arithmetic/text methods. Known-good
  recipes cover the common questions (port, disk, IP, folder sizes, CPU/memory, uptime, versions). A new
  setting `terminalEnabled`, **off by default**, never on in kids mode. Tests: 6 policy tests (a long list
  of refused commands, each checked for the right reason by hand), the recipes against the policy, and two
  **live** ones that run every recipe through the real runner on a PC (`--include-ignored`). Driven in the
  real app: card ("Run this command in PowerShell?"), approve, result card, BYTE explains the output.
- **A real race the terminal test exposed.** The window said "Engine stopped" next to a working engine: it
  asks for the engine's state at startup and also listens for changes, and when a change (ready) arrived
  while the answer was on its way, the older answer overwrote it. A 0.6B model on a fast GPU is ready in
  about a second, so the window lost the race; big models usually win it. `engineAfterLoad()` keeps the newer state.
- **Drive-test trap:** the dev app shares its data folder with the installed app, and an engine left running
  when the driver kills the app can still be there next run. Kill `llama-server*` and clear `byte.db*`,
  `db.key`, `engine.pid` between runs, and wait ~45 s for the first load.
- **Reminders did not understand "in 10 minutes".** BYTE's own reminder parser knew "at 5pm" and "tomorrow at
  9am" but not durations, the commonest way to ask. On a Mac Apple Reminders hides this; on Windows (and
  Linux) nothing did, so "remind me in 10 minutes to X" just got a chat reply and no reminder (the to-do
  list was empty, seen in the real app). `tasks.rs` now reads "in N seconds/minutes/hours/days/weeks",
  "in an hour", "in half an hour", "in three days", before or after the task. Verified in the real app: the
  to-do appears with a bell and its time. **Not verified:** that the Windows toast itself shows (it
  disappears within seconds; one capture missed it); the notification plugin has no action buttons on
  Windows, so the brief's Snooze/Done toast is not built.
- **Driver trap:** Task Scheduler rejects a `/tr` command over 261 characters and the script hid the error, so a
  longer click list silently did nothing for ten minutes. The step list now goes through a file.
- **Disk lesson.** My builds took the PC's C: from 58 GB free to 6 GB in a day
  (several target dirs, incremental caches, downloaded models). One target dir,
  delete `incremental`, check free space before a build. Saved to memory.
- **Trap:** on the PC, `cargo test` rebuilds OpenSSL when RUSTFLAGS differ, which
  needs Strawberry Perl on PATH. Use `scripts/windows-dev.bat`'s PATH.

### 2026-10-08: Files on Windows, second part: the File Explorer selection and converting photos
- **Why:** "summarize the files I selected" and "make the selected photos smaller / convert them to jpg" said "can't yet" on a PC.
- **What:** `WinOp::ExplorerSelection` finds the File Explorer window that was used last (the first one in the window stack) and asks the
  shell for its selected items; `WinOp::ConvertPhoto` saves a copy through Windows' own imaging (WIC via WPF) as JPEG, PNG, TIFF
  or BMP, optionally no larger than N pixels on the long side. A camera's orientation tag is applied (a re-saved photo has no tag
  left, so a phone photo would otherwise come out sideways); JPEG is flattened on white; the camera's other details (location)
  are not copied, and the card says so. Nothing is overwritten (checked in Rust and in the script, and the file is created
  with `CreateNew`), the original is never touched, and a failed conversion leaves no file. With only a size asked, a photo keeps
  its kind when a PC can write it and becomes a PNG otherwise (GIF, WebP, HEIC). HEIC can't be written on Windows, which is said
  plainly. `convert_plan_pc` names the copies like the Mac does ("trip (small).jpg"). Undo of created copies now uses the
  Recycle Bin on Windows (a plain delete if it refuses), so a copy that was edited afterwards can still be fetched back; this
  also applies to notes made from a PC.
- **Verify:** Linux: 625 tests (new: formats and names, the flow with a failing photo and its Undo, HEIC refused, nothing selected,
  an unreadable Explorer, questions about the selection). On the real PC: `live_pc_convert_photo_resizes_turns_upright_and_never_overwrites`
  makes a PNG and a JPEG that the camera says to turn, converts them (400x200 to 200x100 as a real JPEG; the turned one to 50x100
  upright), checks the originals, refuses to overwrite, and refuses bad kinds, sizes, missing and broken photos;
  `live_pc_explorer_selection_reads_the_window_on_top` opens an Explorer window on a temporary folder with one file selected,
  reads exactly that file and closes only that window. HEIC input was not tried (it needs Microsoft's HEIF extension).
- **Not done:** reading the selection on the Desktop itself (icons on the desktop are not an Explorer window), and with several
  Explorer tabs open the first tab Windows lists may be read.
- **Undo:** `git revert`; the Mac path is unchanged.

### 2026-10-08: Files on Windows, first part: "find my tax return" and "organize my Downloads"
- **Why:** `filectl` (find files, tidy a folder, convert the selected photos, ask about the selected files) was macOS-only.
  Finding used Spotlight, tidying was already plain Rust (plan, move, Undo), the other two need Finder and `sips`.
- **What:** Find: `WinOp::FindFiles` asks Windows Search (names, and the text inside documents of the usual kinds), scoped to the
  home folder, folders excluded; the words are letters, digits and `-_.` only (`find_word_ok`) and reach the query through a file.
  `rank_found` puts names with every word at the start of a word first ("2025 Tax Return.pdf"), then names with some, then other
  name matches ("syntax.html" for "tax"), then text matches, newest first inside each, and drops app data, hidden folders
  (`.rustup`), dependency folders, the Recycle Bin and Windows' folders (judged below the home folder). If the search service is
  off, `walk_find` looks through the folders by file name for up to 8 s and says so. Tidy: the folders come from Windows
  (`known_folder`, so a Desktop inside OneDrive is found), Movies is Videos, programs and disk images are Installers, and
  shortcuts, `desktop.ini`, `Thumbs.db`, `~$` Office lock files, hidden/system files and other browsers' partial downloads stay;
  a file open in another program (Windows errors 5, 32, 33) stays where it is and the rest still move, and the card says how
  many were left. The cards say File Explorer, and a folder that can't be read points at Controlled folder access. Asking
  about the File Explorer selection or to convert photos says plainly that BYTE can't yet (next part).
- **Verify:** Linux: 622 tests (new: PC phrases, groups and skipped files, in-use handling with an injected move, parsing and
  ranking of results with 20 kinds of path, the walk, the whole find and tidy flows with Undo). On the real PC (Windows Search is
  running): `live_pc_search_and_known_folders` (all six known folders exist, a made-up word finds nothing, "readme" finds 300
  lines, bad words are refused) and `live_pc_find_a_file_of_this_pc` (0.3-0.6 s, no `.rustup` or AppData noise; it first listed
  folders named Notes for "find the notes document", so folders are now excluded). Not run on real folders: the tidy itself
  (tests do it on temporary folders; it is what the Mac code does). CI's Windows job caught what the live tests could not: on
  Windows the tidy asked the OS for the real Downloads folder instead of the temporary home the test passed in, so that test
  would have tidied whoever ran it. `folder_dir` now asks Windows only when the home it is given is the real home. All 625
  unit tests were then run on the real PC as well (do that, not only the live tests, before pushing).
- **Not done:** the File Explorer selection, converting and shrinking photos (part two).
- **Undo:** `git revert`; the Mac path is unchanged (its tests pass untouched).

### 2026-10-08: PC upkeep, fourth slice: "uninstall X" on Windows
- **Why:** the last of the Mac's upkeep questions that answered "can't yet" on a PC.
- **What:** `WinOp::Programs` lists what is installed (the Uninstall keys of this user, the PC and the 32-bit view, without
  Windows components, updates and sub-entries, plus Microsoft Store apps that can be removed); `WinOp::Uninstall { id }` runs
  the program's own registered uninstaller, found again by `id` in the registry at that moment: the command is never taken
  from the list, from the model or from anything a person wrote, the id goes to the script in a file, and only ids the list
  prints are accepted (`program_id_ok`: `reg|HKCU|<key name>`, `HKLM`, `HKLM32`, or `appx|<package full name>`; no path
  characters, quotes, line breaks). Windows Installer's "change" command is turned into "remove", as Settings does; the
  uninstaller runs in a normal window so its own prompts and Windows' permission prompt show; BYTE waits up to 15 s and
  says `removed` if the entry is gone, `started` if the wizard is still open. A Store app is removed for this user.
  Unlike the Mac there is no Trash to put it back from, so the approval card says first, in plain words, that Undo isn't
  possible and how to get the program back, and the done card has no Undo button. BYTE refuses (and says why) Windows'
  parts, runtimes (Visual C++, .NET, WebView2, DirectX), drivers and hardware software, the user's antivirus, and BYTE itself;
  two matches means it asks which. With this, no upkeep question answers "can't yet" on a PC, so that guard is gone.
- **Verify:** Linux: 613 tests (new: the list parser, titles, matching including exact-beats-contains, the refusals, the id
  gate with 20 bad ids, the whole flow: no, yes, a wizard still open, a Store app, ambiguous / missing / system programs /
  a failing uninstaller / a PC that cannot list). On the real PC: `live_pc_programs_are_listed` reads the real list (171 programs on the owner's PC, ids all accepted by the gate);
  `live_pc_uninstall_runs_the_registered_uninstaller` adds a fake program of its own to the registry whose "uninstaller" removes its
  own entry, runs it through `WinOp::Uninstall` and sees `removed`, then checks that bad ids are refused. No real program is
  ever uninstalled by a test. The script's way of reading uninstall strings was also tried on sample strings on the PC without running
  anything (`/I` becomes `/X`, quoted and unquoted paths, `rundll32`, a missing file, a non-.exe command); that showed bare program
  names such as `rundll32.exe` need resolving through the path, which the script now does. Settings and the Windows help article
  now describe all of PC upkeep (they said more checks were coming).
- **Not done:** removing a program's leftovers in AppData (BYTE does not look inside AppData), file tidying (`filectl`).
- **Undo:** `git revert`; the Mac path is unchanged.

### 2026-10-08: PC upkeep, third slice: "what opens at startup?" and "stop X from opening at startup" on Windows
- **Why:** login items were Mac-only; on a PC those questions answered "can't yet".
- **What:** `WinOp::StartupList` reads what starts with Windows (the user's and the machine's Run keys, the 32-bit Run key, both
  Startup folders, with each entry's on/off state from Windows' own `StartupApproved` flags) as tab-separated lines;
  `WinOp::StartupSet` turns one entry on or off by writing that same per-user flag, the switch Task Manager's Startup tab uses.
  Nothing is deleted: the entry, the program and its files stay, so Undo is the same switch turned back. The kind, name and
  wish go to the script in a file (never on a command line), only the three kinds are accepted, and a name with a line break is
  refused. In `upkeep.rs`: `parse_startup`, `exe_of_command`, `startup_title` (OneDrive, Discord and Steam by their names,
  `MicrosoftEdgeAutoLaunch_<id>` is Microsoft Edge, `electron.app.Name` is Name, `Name.App.0` is Name),
  `match_startup` (by entry name, friendly name or program), and `protected_startup`: Windows' and drivers' entries (anything run
  from the Windows folder, the security tray) and BYTE's own are listed but never offered. The card lists open entries first,
  then the ones already off, and links to Settings → Apps → Startup, because some apps start through scheduled tasks or the Store
  and BYTE says so. Asking to turn one off shows the approval card; "no" changes nothing; an entry that is already off, a
  protected one and an unknown name are explained without asking. Phrases: "with Windows", "when Windows starts", "startup apps".
- **Verify:** Linux: 610 tests (new: a startup list in the shapes the real PC printed, with invented apps, titles, program names in
  commands, matching, protection, the card, approval yes/no, Undo as the reverse switch, entries not changed, a PC that cannot
  read the list). On the real PC: `live_pc_startup_list_and_round_trip` reads the real entries and, on an
  entry it adds itself (with a non-ASCII name), turns it off and on and sees the state change, then removes it;
  `live_pc_the_startup_card_of_this_pc` runs the whole flow.
- **Not done:** uninstalling (next slice), scheduled-task and Store startup apps (Windows' own Settings page covers them), file
  tidying (`filectl`).
- **Undo:** `git revert`; the Mac path is unchanged.

### 2026-10-08: PC upkeep, second slice: "why is my PC slow?" and "check my PC" on Windows
- **Why:** the coach and the check-up (the health card) were Mac-only; on a PC they answered "can't yet".
- **What:** four operations in `pcctl.rs` (`WinOp::Snapshot`, `Processes`, `Security`, plus `Quit`); the first three only read.
  Snapshot is memory, disk, boot time, the Windows version and (laptops only) the battery; Processes is the busiest programs
  with CPU, memory and whether each has a window (sysinfo, two samples); Security asks Windows Security (Defender, firewall
  profiles, encryption of the system drive), the newest installed update, whether a restart is waiting, and battery wear. All
  print plain `key=value` or `name|cpu|bytes|window` lines that `upkeep.rs` parses (`parse_pc_snapshot`,
  `parse_pc_security`, `pc_procs`), so every rule is tested on Linux against output captured on the real PC. `health_pc` asks
  the same questions as the Mac card (storage, memory, busy programs, battery and its wear, last restart; check-up: Windows
  Update, virus protection, firewall, disk encryption, backups) with links into Windows Settings. A program gets a Quit
  button only if it has a window and is not Windows itself, a service, the shell, a terminal, the web view other programs
  share, or BYTE and its engine (`quittable_pc`); the card remembers which program each friendly name means (`PC_QUIT`), so
  the button can only close what the card listed, and `WinOp::Quit` asks the windows to close (WM_CLOSE), never ends the
  process. Another antivirus in charge is shown as information, not an alarm. The slower security reader runs only for the
  check-up (or for a laptop's battery wear), so the coach stays quick.
- **Verify:** Linux: 605 tests (new: parsers on real output, the Quit allow-list including odd Unicode names, the coach and
  the check-up on a busy and a healthy fake PC, a PC that cannot answer, the whole flow in PC words). On the real PC, in the
  desktop session: `live_pc_the_checkup_of_this_pc` runs the real readers (coach 0.6 s, check-up 1.3-1.6 s; it found 22.2 GB
  free of 2 TB and rated it Bad, Defender on and current, firewall on, last update yesterday, device encryption off) and
  `live_pc_quit_asks_a_programs_windows_to_close` opens Notepad, finds its window, asks it to close and sees it go (it
  removes only the Notepads it started). Two bugs only the real PC showed, both in the test: a child that inherits the
  test's output pipe keeps the whole run waiting (now null handles), and the card's list is cut to the busiest programs so
  an idle Notepad is not in it (the test reads the process table). The list now follows what is short: biggest by memory
  when memory is under 25% free, busiest by processor otherwise (the reader returns both sets).
- **Not done:** startup apps, uninstalling (next slices), file tidying (`filectl`).
- **Undo:** `git revert`; the Mac path is unchanged (its tests pass untouched).

### 2026-10-08: PC upkeep, first slice: "what's taking up space?" on Windows
- **Why:** upkeep (storage, what slows the Mac, the check-up, uninstalling, login items) and file tidying were macOS-only
  (`applies` was gated to macOS), so on Windows "what's taking up space?" or "find duplicate files" did nothing.
- **What (storage only):** `upkeep.rs` takes the platform from the runner (`runner.pc()`), like `macctl`. For a PC: the scan
  leaves AppData alone (as Library on a Mac) and finds a PC's developer caches (pip, npm, Yarn, NuGet, Gradle, Cargo);
  Temp is measured and explained, never offered (files in use, and Windows clears them); installers are `.msi/.msix/.appx/.iso`
  and `.exe` files named setup/install; a OneDrive file that is only in the cloud counts as zero (it has a size but takes no
  room here); path rules are string-based (case-insensitive, either slash) so they test anywhere: Windows, Program Files,
  ProgramData, the Recycle Bin, other users and the user's own top folders are never offered or touched, and `..` is refused.
  Removal is the Recycle Bin (`WinOp::Recycle`, never a delete) and Undo is `WinOp::Restore`, which asks the shell's
  "undelete" verb through a fixed script (the paths go in a file, never on a command line). "Show in File Explorer" is
  `WinOp::Reveal`. The cards say Recycle Bin / Restore on Windows (word swaps in TS `osText` and Rust `os_text_for`; Linux
  keeps Trash). The other questions (slow, check-up, startup apps, uninstall) answer that they can't yet on a PC.
  A "PC upkeep" switch appears in Settings on Windows.
- **Verify:** 600 Rust tests (new: sentences in PC words and `pc` as a whole word, offerable/protected paths, installers, the
  scan on a fixture profile, the card, the Recycle Bin step with partial failure and its Undo step, protected paths never
  reaching the runner, the whole flow with a fake PC runner). On the real PC, in the desktop session: a throwaway .msi went
  into the actual Recycle Bin and came back unchanged through Restore (a missing file was reported, not hidden), and the scan
  of the real profile took 28 s (20 s walk + 15 s copies, within limits) and found: 22.8 GB free of 2 TB, Downloads 5.7 GB,
  the NVIDIA app installer saved three times (476 MB), 15 old installers (249 MB), the Cargo and npm caches, 838 MB of temp.
  Two of my tests failed on Windows at first for reasons only a Windows run shows (a Unix path used for a Mac rule; the temp
  folder the test runs in is itself inside AppData): fixed in the tests.
- **Not done:** the coach, the check-up, startup apps, uninstalling (next slices) and file tidying (filectl).
- **Undo:** `git revert`; the Mac path is unchanged (its tests pass untouched).

### 2026-10-08: Linux, first slice: the app runs, sees the GPU, downloads a model and chats
- **Why:** Linux is the next target after Windows and Android (`docs/PORTING-WINDOWS-LINUX.md`, L1). Until now it compiled
  (CI runs the Rust tests on Linux) but every Linux machine looked like it had no graphics card, the engine script built a
  native-CPU-only binary, the tray library missing at run time aborted the whole app, and every screen said "Mac".
- **What:** `gpu_linux.rs` reads the cards from `/sys/class/drm`, `nvidia-smi` and `lspci` (pure parsers over text from real
  machines: 14 tests; an RTX 4060 Ti, a Radeon, Intel graphics, hybrid laptops, VMs). `build-llama-server.sh` has a Linux
  branch (portable CPU baseline `GGML_NATIVE=OFF`, RUNPATH `$ORIGIN/../lib/byte`, C++ runtime linked in, CUDA / Vulkan / CPU as
  separate sidecars; a check that the binary links nothing a clean machine lacks). `gpu::engine_sidecar` maps Vulkan to
  `llama-server-vulkan` and the processor to `llama-server-cpu` on Linux (the default CUDA build cannot start without
  NVIDIA's `libcuda`). `syslib.rs` asks whether a shared library is installed; the tray and the Vulkan check use it, so a
  desktop without the appindicator library simply has no tray icon instead of aborting. Quick Ask defaults to Ctrl+Alt+Space
  on Linux (Alt+Space is the window menu of GNOME, KDE and most window managers).
  **Wording:** a third platform. `Os` (Rust `platform_text`, TS `platform.ts`), help-article blocks `<!-- linux -->` and
  `<!-- pc -->` (Windows or Linux), Linux word swaps (file manager, system keyring, account password), key glyphs follow
  "not a Mac". `isLinux()` needs a WebKit user agent (Node's `navigator.platform` is "linux" too, and Android says Linux).
  Help text only says what Linux does today (no hotkey, clipboard or PC control yet); tests guarantee a Linux reader is
  never shown a Mac or a Windows step.
  **Secrets:** the cloud key, connector tokens and the weekly-backup passphrase now live in the desktop's keyring on Linux
  (Secret Service over D-Bus, `keyring` with the pure-Rust zbus backend, so no libdbus is needed at run time); with none
  running the error says to install GNOME Keyring or KDE Wallet. Tested for real against GNOME Keyring in a desktop-style
  session (create the login keyring, save, read, replace, delete): `cargo test --lib secret_service_round_trips -- --ignored`.
  **Packaging:** `tauri.linux.conf.json` (a .deb and an AppImage; the package depends on WebKitGTK, GTK and the tray library
  and recommends Vulkan and a keyring) and `tauri.linux-release.conf.json` (the six sidecars). A debug .deb was built here
  (`npm run tauri build -- --debug --bundles deb --config src-tauri/tauri.linux-release.conf.json`), unpacked and run: the app,
  the three engine variants and the icon land in `usr/bin` and the app starts, finds its saved chat and loads the model.
  **A crash found by running the package:** the keyring's `tokio` feature blocks on the app's own tokio runtime, which panics
  ("Cannot start a runtime from within a runtime"); a release build is `panic = "abort"`, so it would have killed the whole app
  at startup. Fixed by using the `async-io` feature. The test `the_keyring_works_from_inside_the_async_runtime` fails with the
  old setting (checked) and passes with the new one.
  **Shared GPU (a real bug found on this machine, where the cluster's own engine holds 10 of 16 GB):** the budget came from the
  card's total memory, so a model that did not fit what was free failed to start and the retries could not know why.
  `SystemInfo::with_free_vram` plans a LOAD from the memory free now (NVIDIA through `nvidia-smi`, AMD through sysfs), adding
  back what BYTE's own engine holds (asked of the driver by process id), and lets a chosen dense model run up to 80% on the
  processor (`SHARED_CARD_STRETCH`; the usual cap of 15% stays for recommendations). A restart that cannot be planned no longer
  marks a working engine as failed (the tuner's optional restart did exactly that and left the screen red over a running engine).
- **Verify:** 589 Rust and 268 front-end tests. On this machine (Ubuntu 24.04, RTX 4060 Ti, headless with Xvfb): the CPU and Vulkan
  engines built and answered through BYTE's own chat client (66 and 238 tokens/s on a 0.6B model); the whole app started,
  showed the welcome and "Checking your PC" screens with the real Ryzen 9 7900X / RTX 4060 Ti 16 GB / 30 GB / 392 GB, offered
  models, downloaded Qwen3.5 9B (7.5 GB) with its own downloader, loaded it split across the GPU and processor beside the
  cluster's engine, titled the chat itself and answered at 9.8 tokens/s; Settings opens with Ctrl+,.
- **Not done yet (each is a later slice):** the CUDA engine (no `nvcc` here; CI will build it), whisper and sherpa for Linux,
  OCR (Tesseract), the lock, clipboard history, the selected-text hotkey, voice and spoken answers,
  PC control, autostart, packaging (.deb and AppImage) and the release job, glibc floor (build on Ubuntu 22.04 in CI).
- **Traps:** `pkill -f` / `pgrep -f` with a pattern that also appears in your own command line kills or matches your own
  shell. Tauri copies sidecars into `target/debug/` at build time and a later `chmod` of the source does not reach the copy.
  The app asks for the tray library at the moment the icon is made, not at start.
- **Undo:** `git revert` this commit; Mac and Windows behaviour is unchanged (their tests pass untouched).

### 2026-10-08: PC control, the Windows counterpart of Mac control (`pcctl.rs`)
- **Why:** on Windows the whole "BYTE does things on your computer" feature was off (`macctl::applies` and the
  prompt were gated to macOS), and the help text said it "isn't available on Windows yet". W3 of
  `docs/PORTING-WINDOWS-LINUX.md`.
- **What:** `macctl.rs` keeps the sentence reader, approval card, result card and notes for the model; only the
  last step differs. New `src-tauri/src/pcctl.rs`: `WinOp` (pure data) and a `WinRunner`; `macctl::Command` gains
  `Win(WinOp)`, `Runner` gains `pc()`, and `Action` gains `app_for(pc)` / `describe_for(pc)`. Nothing the Mac does
  changed: every Mac test passes unmodified (two test helpers got a `Win` arm). On a PC:
  dark mode (registry + `WM_SETTINGCHANGE`), volume and mute (Core Audio `IAudioEndpointVolume`), Wi-Fi (the
  `Windows.Devices.Radios` API), display off (`SC_MONITORPOWER`), Settings pages (`ms-settings:` addresses, 60 keys),
  music (the system media session: pause, resume, next, previous, what's playing; "play X" opens a Spotify search,
  else YouTube Music), an email draft (`mailto:`), a calendar entry (an .ics file the calendar app opens and asks to
  save). Notes go to BYTE's own Notes (with Undo), reminders to BYTE's Tasks (already routed before this module).
  Said plainly instead of attempted: texts (Phone Link has no API), Shortcuts, the browser's current tab, reading the
  inbox or calendar. No Contacts app, so an email needs an address. The user's words are never part of a command
  line: each operation is a Rust function that receives them as data (tested with hostile text).
  Also: `prompt::PC_CONTROL`, the help article's Windows block, the Settings switch text ("PC control, hotkeys and
  clipboard"), `Cargo.toml` (windows crate features for audio, registry, COM, radios, media session).
- **Verify:** 13 `pcctl` tests + 8 PC flow tests in `macctl_tests.rs` (run anywhere; the flow tests drive the real
  `run_with` with a fake PC runner); 569 Rust tests and 257 front-end tests pass. On the real PC (Windows 11, a
  logged-in desktop session; `cargo test --lib live_pc -- --ignored --test-threads=1`): volume set to 37/12/20 and
  read back from the mixer, mute and the unmute-on-volume rule, dark mode on/off/toggle read back from the registry,
  the Sound page of Settings opened (`SystemSettings.exe` in the console session), the Wi-Fi radio found, and a
  looping tone in a throwaway Edge profile read as "now playing", paused (session reports stopped) and resumed. Each
  live test restores what it changed.
- **Not verified:** a real mail app opening a `mailto:` draft or a calendar app importing the .ics (no mail or
  calendar app was set up on the test PC), display-off (would blank the screen mid-session), Wi-Fi off (would cut the
  SSH link the tests use), Spotify (installed, not signed in), and the whole thing through the chat window with a
  model (only the module and its flow tests).
- **Trap:** `cargo test` on Windows still needs the two `comctl32` linker flags (see "Running the Rust tests on
  Windows"); without them the test exe dies with 0xC0000139 and the real error is hidden by an output filter.
  Also `windows::Foundation::IAsyncOperation` is not where it was: use `.and_then(|o| o.join())` and never name it.
- **Undo:** `git revert` this commit. `pcctl.rs` is additive; the Mac path is unchanged.

### Still open
(Written 2026-10-04; several items are done since, see the later entries above.)
- **The layout is device-class, not continuous.** Breakpoints stop at 560px; a
  folded cover screen is ~320px and Android split-screen is arbitrary. The owner
  wants it to adapt to any screen including foldables, which needs container
  queries and fluid scales rather than more breakpoints. **Do this before any
  Android UI work or it gets built twice.**
- Voice engines (`whisper-cli`, `sherpa-*`) are zero-byte placeholders on
  Windows, so voice does not work; chat does.
- No installer yet, and `release.yml` has no Windows job.
- `titleBarStyle: Overlay` and `hiddenTitle` are macOS-only, so Windows draws a
  standard titlebar where the Mac runs content under the window controls. Needs
  a `tauri.windows.conf.json` overlay.

---

## 2026-10-04

### (this commit) — "same look everywhere" carried into CLAUDE.md, HANDOFF and the Android brief (docs only)
- **Why:** the owner's 2026-10-04 instruction (recorded by the cluster session in `228d5aa`): the same look as the
  Mac app on every platform. `CLAUDE.md` and `HANDOFF.md` §1b still said "feel native rather than identical".
- **What:** `CLAUDE.md`, `docs/HANDOFF.md`, `docs/ANDROID.md` ground rules.
- **Undo:** `git revert` this commit.

### (this commit) — Android handover brief; keystores blocked from the repo
- **Why:** owner: hand Android to the cluster session (which also reaches the PC) with everything it could need.
- **What:**
  - `docs/ANDROID.md` is now a full brief: who it's for and the read order; ground rules (branch
    `claude/android-port`, cfg rules, no secrets); the phone and adb; Tauri 2 Android, with desktop-only plugins
    under `cfg(desktop)` and Android APIs in a Kotlin plugin.
  - The engine: `llama-server` built with the NDK and run from `nativeLibraryDir`, with a library fallback.
    Plus the chip table, the catalog phone pass, setup, keystore + secrets + in-app updater + `release.yml` job,
    milestones A1–A5, testing and reporting.
  - `CLUSTER-REQUESTS.md` item 0 points to it.
  - `scripts/check-secrets.sh` fails on tracked `*.jks/*.keystore/*.p12/*.pfx/*.key`, and `.gitignore` ignores
    them.
- **Verify:** `scripts/check-secrets.sh` passes; a force-added `x.jks` makes it fail.
- **Undo:** `git revert` this commit.

### (this commit) — Android AI focus only while BYTE or Ask BYTE is in use (docs only)
- **Why:** owner: AI focus applies "when the app is open or when using Ask BYTE".
- **What:** `docs/ANDROID.md`:
  - AI focus is active while BYTE is on screen or Ask BYTE is in use, and while an answer is finishing;
  - the model preloads when BYTE opens;
  - after leaving, BYTE unloads after a 2-minute grace period and the RAM goes back.
- **Undo:** `git revert` this commit.

### (this commit) — Android "AI focus": other apps go to RAM Plus, the AI keeps the RAM (docs only)
- **Why:** owner: let other apps stay open in storage (RAM Plus) while the AI is on, so RAM is mainly for the AI.
- **What:** `docs/ANDROID.md`:
  - the AI focus setting;
  - counting what background apps can give up, from the per-phone memory test with RAM Plus on;
  - the trade-off text, and RAM returned when BYTE closes;
  - never closing other apps, and no `mlock`;
  - keeping RAM Plus on.
- **Undo:** `git revert` this commit.

### (this commit) — Android: use all the RAM it safely can; RAM Plus handled honestly (docs only)
- **Why:** owner: Android should use as much RAM as it physically can without problems, for max use and speed, and
  asked about the phone's setting that uses storage as RAM.
- **What:** `docs/ANDROID.md`:
  - a live memory budget instead of a fixed cap, with backing off on pressure, a foreground service and a
    per-phone memory test;
  - memory-mapped weights, big-core threads, and the GPU when it's faster;
  - RAM Plus detected but not counted as RAM, with an opt-in labelled Stretch for slightly bigger models.
- **Undo:** `git revert` this commit.

### (this commit) — Android extras added to the plan (docs only)
- **Why:** owner asked what else Android could do, then "add all of it to the notes".
- **What:** `docs/ANDROID.md`, a new "Extras only Android can do" section and milestone A4:
  - default assistant, Ask BYTE on selected text, replies in any messaging app, what's on my screen;
  - camera: point and ask, live translate, scanner;
  - Fold: interpreter mode, Flex mode, split screen;
  - location reminders, phone actions, call screening, earbuds, no-signal mode;
  - the honest limits.
- **Undo:** `git revert` this commit.

### (this commit) — platform order: Windows → Android → Linux (docs only)
- **Why:** owner: "after windows we can do it [Android] then linux".
- **What:** order updated in `CLAUDE.md`, `ANDROID.md`, `HANDOFF.md` §1b, `VERSIONS.md`, `PORTING-WINDOWS-LINUX.md`,
  and the v1.0 draft notes.
- **Undo:** `git revert` this commit.

### (this commit) — Android planned (docs only)
- **Why:** owner: an Android build for their Galaxy Z Fold8 Ultra, for phones up to 16 GB of RAM (the desktop catalog
  goes to 128 GB), with the catalog expanded in that range.
- **What:** `docs/ANDROID.md` (new: plan, catalog rule, reuse via Tauri 2 Android, feature map, shipping, milestones);
  the platform decision in `CLAUDE.md`, `HANDOFF.md` §1b, `DESIGN-AND-PLATFORMS.md`; `VERSIONS.md` and the v1.0
  draft notes' "Coming next".
- **Verify:** read the files. Nothing is built.
- **Undo:** `git revert` this commit.

### (this commit) — v1.0 release notes drafted (docs only)
- **Why:** v0.12.6 is out, and the owner is testing it with `docs/CHECKLIST-1.0.md`. The 1.0 notes are written now
  so 1.0 can ship as soon as the checklist passes.
- **What:** `docs/releases/v1.0.0.md` (a draft, with an HTML comment to remove when it's finalized);
  `docs/VERSIONS.md` "Coming next" brought up to date (v0.12.7+ fixes, v1.0.0, Windows/Linux, v1.1 images).
- **Verify:** read both files; nothing is bumped or released.
- **Undo:** `git revert` this commit.

### 144ae1b — email replies answer instead of copying
- **Why:** the v0.12.6 Mac test failed `e2e_mail_reply_draft`: Qwen3 0.6B returned Sam's own email as the reply
  ("reply to Sam's email saying I can make it…"). A real bug, not a flake: nothing checked that a reply answers.
- **What:** `macctl::reply_answers` (greeting/sign-off lines skipped; at most half the reply's 4-word runs may be in
  their email), up to 3 tries with "don't repeat their email", then a reply built from `user_words(q)`; test.
- **Verify:** `cargo test macctl`; the real-model test passed 3/3 locally (llama-server + Qwen3 0.6B).
- **Undo:** `git revert 144ae1b`.


### (this commit) — stories stay stories
- **Why:** owner: asking for a story sometimes started a Mac action. The rules didn't fire on 30+ probed story
  prompts, so the guard is general: a creative-only request never reaches computer control.
- **What:** `router::creative_only` (+ test); `agent::specialist` uses it to switch off Mac control, files, upkeep,
  terminal and automations for that turn; `backend` leaves out `prompt::MAC_CONTROL` then.
- **Verify:** `cargo test creative`; on the Mac, "make a story about a girl who texts her mom" just writes a story.
- **Undo:** `git revert` this commit.


### (this commit) — v0.12.6: texting from BYTE, and a Messages inbox
- **Why:** owner: texts should send after "Do it" (showing it sending), with suggested edits, Fix grammar &
  punctuation, Rephrase with tones (formal, fun, sympathetic…), and a place for received messages with "Draft a
  reply" and "Reply". Chose: send after approval; inbox with Full Disk Access; ship before 1.0.
- **What:** `macctl.rs` MessageSend + MESSAGE_SEND (chat id, else iMessage, else SMS; draft fallback), editable
  approval fields (`web_agent` answer_with/take_edits, `agent_approve` edits); `messages.rs` (chat.db read-only,
  attributedBody, Contacts names, threads/thread/send, watcher), settings `messagesInbox`/`messagesNotify`,
  Full Disk Access permission row; UI `MessageComposer`, `MessagesPanel`, new-text banner, Privacy section,
  "Send" label; tones fun/sympathetic; prompt MAC_CONTROL wording; help 07, checklist 11a/11b, CLAUDE.md layout;
  ui-shots `44-*`; release notes, VERSIONS, CHANGELOG; version 0.12.6.
- **Verify:** check-all (499 Rust incl. messages fixture and send/edit/fallback flows, 217 vitest); on the Mac:
  text card → Send arrives; inbox banner → Draft a reply → Send.
- **Undo:** `git revert` this commit.


### (this commit) — port brief: texting on Windows through Phone Link
- **Why:** owner: Windows should send and receive texts through Phone Link, which already links the phone.
- **What:** `PORTING-WINDOWS-LINUX.md`: the texting row (Phone Link / KDE Connect) and a "Texting through Phone Link"
  section (reading its local cache read-only or its notifications, sending through UI Automation after approval,
  iPhone limits, unofficial-API fallback, shared UI).
- **Undo:** `git revert` this commit.


### (this commit) — the microphone: entitlements under the hardened runtime (v0.12.5)
- **Why:** owner: BYTE asks for the mic, but isn't in System Settings → Microphone (screenshots). Tauri signs with
  the hardened runtime; without `com.apple.security.device.audio-input` macOS denies silently and never lists the
  app. Apple Events need `com.apple.security.automation.apple-events` the same way.
- **What:** `src-tauri/Entitlements.plist`, `tauri.conf.json` `bundle.macOS.entitlements` (+ explicit
  `hardenedRuntime`), test `privacy::mac_permissions_are_declared`, `release.yml` codesign check, help 14, v0.12.5
  notes, CHANGELOG.
- **Verify:** `cargo test mac_permissions`; release run's verify step; on the Mac the mic prompt appears and BYTE is
  listed under Microphone.
- **Also:** `src/a11y.test.ts` scanned from the repo root (node_modules and the 20 GB Rust build folder), so it
  timed out as the build grew; it now scans `src/` only (~1 s).
- **Undo:** `git revert` this commit.

## 2026-10-02

### (this commit) — v0.12.5: read-only updates explained, clean notes, install steps
- **Why:** owner's one-click update from v0.12.3 failed: "Read-only file system (os error 30)". macOS runs an
  unsigned, quarantined app from a read-only App Translocation copy (or the .dmg). The notes also showed raw `**`.
- **What:** `updater::unwritable_place` + mapping of os error 30 to `MOVE_HELP` (tests); `lib/updateNotes.ts`
  (+ test) in `UpdateRow`; README, `release.yml` footer and help 14 say to run `xattr -cr /Applications/BYTE.app`
  before the first launch; release notes v0.12.5, VERSIONS, CHANGELOG; version 0.12.5.
- **Verify:** `cargo test updater`; owner: xattr once, then v0.12.3 → Install 0.12.5 works.
- **Undo:** `git revert` this commit.


### (this commit) — "text Mom I'm on my way" works without "saying"; BYTE never says it can't text
- **Why:** owner tried "Text mom this is ai sending this message": no Messages card (routing needed "that"/"saying"/":"),
  and the model answered "I can't actually send text messages".
- **What:** `macctl.rs`: `plain_text()` (person = family word, capitalized name, phone number or email; the rest is
  the message), `after_first()` (earliest cue wins), shared `TEXT_CUES`/`SEPARATORS`; routing accepts the plain form.
  `prompt::MAC_CONTROL` (macOS, Mac control on) tells the model what BYTE can do and to point to direct requests.
  Tests: routing positives/negatives, `plain_texts_split_person_and_message`, a flow test with the owner's words.
- **Verify:** `cargo test macctl prompt`; on the Mac, "text Mom I'm on my way" → card → Messages filled in.
- **Undo:** `git revert` this commit.


### (this commit) — port brief: reuse first, don't rewrite
- **Why:** owner: most of the Mac app can be reused; it needs a few changes, not a rewrite.
- **What:** `PORTING-WINDOWS-LINUX.md` rule: same codebase, platform branches inside existing modules, extend the
  planner/estimates/tuner instead of copying, one UI with OS-aware wording; new files only for new things.
- **Undo:** `git revert` this commit.


### (this commit) — port brief: speeds per model, like the Mac
- **Why:** owner: the PC apps must know each model's speed like the Mac does.
- **What:** `PORTING-WINDOWS-LINUX.md` "Speeds per model": the Mac's `chip::estimate` method, the split formula
  for VRAM + RAM (per-device bandwidth), MoE expert offload, hardware tables for all vendors, measured speed stored
  per version + memory mode + backend, and every place the speed must show.
- **Undo:** `git revert` this commit.


### (this commit) — drop the "BYTE Home" idea: separate apps per OS
- **Why:** owner: no linking the Mac and the PC for now; separate OS apps only.
- **What:** `PORTING-WINDOWS-LINUX.md` replaces the idea with "Not in scope: linking devices"; HANDOFF §1b notes it.
- **Undo:** `git revert` this commit.


### (this commit) — port brief: every Mac feature, and more
- **Why:** owner: Windows and Linux should have almost every Mac feature if not all, and more, since macOS is the
  most locked down.
- **What:** `PORTING-WINDOWS-LINUX.md` opens with that rule and a parity checklist of every feature area (shared
  code vs native, a box per OS); CLUSTER-REQUESTS item 0 states the rule.
- **Undo:** `git revert` this commit.


### (this commit) — port brief: any GPU vendor, hardware smarts, memory modes
- **Why:** owner: Windows/Linux must work on AMD and Intel graphics too, be as hardware-aware as the Mac app
  (detection, estimates, measured speed, per-model tuning), and run bigger models across GPU + CPU + RAM + VRAM,
  or each separately, as the user chooses.
- **What:** `PORTING-WINDOWS-LINUX.md` "Any hardware" section: what the Mac does (chip.rs, system.rs, speed.rs,
  tune.rs, modelcfg.rs), per-vendor detection and backends, the four memory modes, fit/estimates per mode, tuning
  on PCs, multi-GPU, re-planning, and a test matrix.
- **Undo:** `git revert` this commit.


### (this commit) — port feature map, and the port listed in CLUSTER-REQUESTS.md
- **Why:** the owner's PC session reads `docs/CLUSTER-REQUESTS.md`; owner wants every Mac feature mapped to Windows
  and Linux, plus what those can do that the Mac can't.
- **What:** `CLUSTER-REQUESTS.md` item 0 points the PC session at the port brief; `PORTING-WINDOWS-LINUX.md` gains
  the feature map (Mac → Windows → Linux), Windows-only and Linux-only extras, and the "BYTE Home" idea (ask first).
- **Undo:** `git revert` this commit.


### (this commit) — brief for the Windows/Linux port on the owner's PC
- **Why:** the owner has a Claude Code session that can SSH into their dual-boot PC (Ryzen 7 7800X3D, 32 GB
  DDR5-6000, RTX 5080) and wants it to build the Windows app, then Linux.
- **What:** `docs/PORTING-WINDOWS-LINUX.md` (machine, ground rules incl. its own branch, setup, Mac-only inventory,
  milestones W1–W4 and L1–L4, reporting); pointer at the top of `CLAUDE.md`.
- **Undo:** `git revert` this commit.


### (this commit) — v0.12.4 release notes and version
- **Why:** ship the polish pass; it's the first version v0.12.3 installs through the updater.
- **What:** `docs/releases/v0.12.4.md`, VERSIONS, CHANGELOG, PROJECT_GUIDE, HANDOFF; version 0.12.4 (bump.mjs,
  incl. `Casks/byte.rb`).
- **Verify:** release has `latest.json` 0.12.4 signed with key 360A24738B9BC87B; v0.12.3 → About → Install works.
- **Undo:** `git revert` this commit.


### (this commit) — owner asks: grown-up models out of kids mode, documents marked Beta, BYTE key steps
- **Why:** owner: keep the uncensored/"obliterated" models (they stay in the catalog) but exclude them from kids
  mode; mark file making Beta (PowerPoint needs work); explain where the BYTE key is (make an account on the
  website, Settings, bottom of the page).
- **What:** `kids::grown_up_model` (community remixes and anything tagged uncensored: role-play and dark-story
  remixes aren't always tagged) refused in `backend::local_turn` while kids mode is on, and `kids_enter` refuses
  while one is loaded (test). `documents/BetaTag.tsx` on both document panels' titles and the Slides choice,
  a beta note, "Save as PowerPoint (beta)", "Documents (beta)" in the top bar and palette.
  `settings/CloudKeySteps.tsx` (link to byteai.bytebylogan.xyz + 3 steps) in Settings → Cloud and the welcome
  guide; help article and CLOUD-MODE.md updated. Screenshot script follows the new labels.
- **Verify:** `cargo test kids`; screenshots `10-cloud-connect`, `11-docs-create`, `15-docs-local`.
- **Undo:** `git revert` this commit.


### (this commit) — v1.0 polish, part 3: panels load on first use (startup code 985 → 695 KB)
- **Why:** the startup script held every panel (Settings, Notes, Board, Help with its articles, Study, Writing,
  Documents, Tasks, Reader …) though most are opened rarely.
- **What:** `Shell.tsx` loads 14 panels with `React.lazy` inside one `Suspense` (fallback none); the Settings tab
  list moved to `components/settings/tabs.ts` so the palette needn't load Settings.
- **Verify:** `npm run build` (index chunk ~695 KB); all 146 `tools/ui-shots` screens render with no errors.
- **Undo:** `git revert` this commit.


### (this commit) — v1.0 polish, part 2: model descriptions, welcome-guide order, README screenshots
- **Why:** the welcome guide showed "Ornith models." as a model's description; 23 chat models had placeholder
  family blurbs ("Zhipu's GLM models.", "Ornith models."). Community remixes (uncensored etc.) could rank in the
  guide's top four. The README had no pictures and no honest limits.
- **What:** real blurbs in `scripts/discover-models.mjs` (source) and the generated catalogs; `runnable()` ranks
  community/uncensored models after squeezed ones (test); README "A look inside" (six screens in `docs/images/`,
  256-colour PNGs, ~75 KB each) and "What it can't do (yet)".
- **Verify:** check-all (487 Rust, 211 vitest); `tools/ui-shots` `03-choose-model`.
- **Undo:** `git revert` this commit.


### (this commit) — v1.0 bug hunt, part 1: welcome guide picks, kids-mode sidebar, screenshots
- **Why:** owner chose a polish pass (bug hunt, first run, speed/memory, README) before v1.0. All 146 mocked
  screens render with no console errors; these are the problems found by looking at them.
- **What:** `Onboarding.tsx` `runnable()` lists versions of 4+ bits before squeezed ones (the guide's #2 pick was a
  2-bit 27B, "Low quality"), with `runnable.test.ts`; `Sidebar.tsx` hides Projects and the private-chat button in
  kids mode; `tools/ui-shots/shots.mjs`: download mock matches the 7.5 GB card, the chat shot asks the question its
  answer is about, update mocks + shot `43-update-found`.
- **Verify:** `npx vitest run src/components/onboarding`; `node tools/ui-shots/shots.mjs` → `03-choose-model`, `41b-kids`.
- **Undo:** `git revert` this commit.


### (this commit) — releases check the updater key before using it
- **Why:** the v0.12.3 release (run 37000452819) built the app, then failed: "failed to decode secret key …
  Invalid padding". The `TAURI_SIGNING_PRIVATE_KEY` secret was damaged when copied from a terminal.
- **What:** `scripts/updater-key.sh` (used by `release.yml`): strips spaces and line breaks, checks the key decodes
  to a minisign secret key and that the password secret unlocks it (signs a scrap file), then passes it to the build
  through `$GITHUB_ENV` (masked). A missing or unreadable key or a wrong password gives a warning saying what to fix,
  and the release is built without update files instead of failing.
- **Verify:** tested with a throwaway key: whole, wrapped, cut-off, empty, right and wrong password.
- **Undo:** `git revert` this commit.


### (this commit) — v0.12.1 and v0.12.2 ship inside v0.12.3
- **Why:** the v0.12.1 release failed twice at "create release" ("Resource not accessible by integration"), the
  second time with nothing pushed during the run. The pattern across v0.10.0, v0.11.2 and v0.12.1: the commit being
  released wasn't the branch head anymore. Releasing the head works.
- **What:** release notes for v0.12.1 and v0.12.3 say so; HANDOFF records the rule (release only the branch head).
  v0.12.1's .dmg stays available as the run artifact `BYTE-v0.12.1-dmg` (run 36960000939).
- **Undo:** `git revert` this commit.


### (this commit) — v0.12.3: the owner's update key, one-click updates on
- **Why:** the owner made the signing key on their own machine, added the private half and its password as GitHub
  secrets, and sent the public half. With it in the app, BYTE can verify and install its own updates.
- **What:** `tauri.conf.json` `plugins.updater.pubkey` (minisign key id 360A24738B9BC87B), a test that pins it,
  `docs/releases/v0.12.3.md`, VERSIONS, CHANGELOG, PROJECT_GUIDE, HANDOFF, help article 01; version 0.12.3.
- **Verify:** the v0.12.3 release has `latest.json` and `BYTE.app.tar.gz(.sig)`; the next release shows up in
  Settings → About → Updates on a Mac with v0.12.3 and installs.
- **Undo:** `git revert` this commit (builds without the key hide the Updates row).


### (this commit) — updates, Homebrew and the README, ahead of v0.12.3 (not released yet)
- **Why:** Phase 12's distribution items. Owner chose one-click signed updates; the signing key is created on the
  owner's Mac (private half only as GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD`, never here), and the
  public half goes into `tauri.conf.json` → `plugins.updater.pubkey` before v0.12.3 is released.
- **What:** `updater.rs` (`update_configured`, `update_check`, `update_install`, daily check → `update://available`,
  tests), `tauri-plugin-updater`, `plugins.updater` endpoint (latest release's `latest.json`, empty pubkey for now),
  setting `updateCheck`, scheduler daily hook; `release.yml` builds signed update files only when the secret exists;
  UI `components/update/UpdateRow.tsx` (hidden until the build has a key), Shell banner, types/api; `Casks/byte.rb`
  (this repo is its own tap; postflight clears quarantine) kept in step by `scripts/bump.mjs`; README rewritten
  (features, Homebrew, updates, roadmap).
- **Verify:** `cargo test updater`; `node scripts/bump.mjs --check 0.12.2`; after the key is set: a release has
  `latest.json`, and Settings → About → Check for updates finds the next one.
- **Undo:** `git revert` this commit.


### (this commit) — v0.12.2: theme editor, smoother motion, Reduce motion, sounds, accessibility check
- **Why:** Phase 12's looks items (owner's list: theme editor with import/export, 60 fps animations that respect
  reduced motion, optional sounds, accessibility pass).
- **What:** `lib/color.ts`, `lib/customTheme.ts` (+ test), `lib/sounds.ts`, `components/settings/ThemeEditor.tsx`,
  `SettingsModal.tsx` (CustomThemes, Reduce motion, Sounds), `App.tsx` (`useAppearance` applies custom themes and
  `data-motion`), `store.ts` (chime on done), `MessageView.tsx` (`streaming` class), CSS (motion, editor);
  `settings.rs` (`customThemes` validated, `reduceMotion`, `sounds` + test); `src/a11y.test.ts`; help article 01;
  screenshots `42-*`.
- **Verify:** `npx vitest run src/a11y.test.ts src/lib/customTheme.test.ts`; `cargo test settings`; on a Mac:
  Settings → Appearance → Make your own, Save and use, Export then Import; Reduce motion; Sounds then ask something.
- **Undo:** `git revert` this commit (custom themes fall back to Midnight in older builds).


### (this commit) — v0.12.1: kids mode, encrypted backups, auto-delete, erase everything
- **Why:** the rest of Phase 12's privacy items.
- **What:** `kids.rs` (PIN hashing, backoff, `PROMPT`, `harmless`, `is_on`/`may_touch`/`grownups_only`, commands,
  tests), `backup.rs` (seal/open, archive, stage/apply restore, erase, list/prune, auto-delete, daily, commands,
  tests including a real SQLCipher backup → restore), `backend.rs` (kids enforcement), `commands.rs` (kids checks in
  chat commands and `settings_update`), grown-up-only gates in notes/board/clipboard/jobs/tasks/trackers/dashboard/
  privacy, `scheduler.rs` (daily chores), `lib.rs` (apply_pending before the DB opens, commands), `notes::dir_for`,
  `lock::verify`, settings (kids, backup, autoDeleteDays), Cargo `ring` + `walkdir` (already in the lockfile). UI:
  `PrivacyTab.tsx` (Kids mode, Backups, Old chats, Erase everything), `components/kids/KidsExit.tsx`, Shell (kids top
  bar, keys), Sidebar (no projects in kids mode), EmptyState + `KID_EXAMPLES`, `lib/privacy.ts` (+ test), CSS, help
  article 11; screenshots `41-*`.
- **Verify:** `cargo test backup kids`; `npx vitest run src/lib/privacy.test.ts`; on a Mac: Back up now (file appears in
  iCloud Drive → BYTE Backups), Restore… it (BYTE restarts with the same chats); turn on kids mode (only Kids chats,
  "how do I hack…" gets a gentle answer, Grown-ups + PIN turns it off).
- **Undo:** `git revert` this commit (old builds ignore the new settings; a "Kids" folder may remain).


### (this commit) — v0.12.0: offline switch, Touch ID lock, Privacy tab (Phase 12 starts)
- **Why:** Phase 12's first items: one switch that keeps BYTE off the internet, a lock, and a permissions dashboard
  with the action log (`ActionLog` was written since Phase 2 but never shown).
- **What:** `offline.rs` (flag, `guarded` connector layer, `OfflineAwareResolver`, tests with their own switch),
  `tools/fetch.rs` (`web_client` guarded, resolver check), `cloud/mod.rs`, `lab.rs`, `backend.rs` (web off; cloud →
  local), `models.rs` (downloads), `watchers.rs`, `commands.rs` (catalog refresh, settings apply, lock check),
  `web_agent/browser.rs`; `lock.rs` (+ tests, mac `can_check` test), `lock::ensure` in the data commands; `privacy.rs`
  (+ tests); `quick.rs` tray Offline/Lock; settings `offline`, `lockEnabled`, `lockAfterMinutes`; Cargo deps
  `tower-layer`, `tower-service`, `objc2-local-authentication`, `objc2-av-foundation` (macOS);
  `mac-engine.yml` paths. UI: `LockScreen.tsx` (+ `useLock`), `App.tsx` gate, `PrivacyTab.tsx`, `lib/privacy.ts`
  (+ test), Shell (pill, palette, listeners), Composer (Web: Offline), types/api, CSS; help article 11; screenshots
  `40-*`. `help.rs` `best` now ignores words found in most articles ("byte", "open"), which the longer privacy article
  exposed. Mac-only code type-checked with a scratch crate (`cargo check --target aarch64-apple-darwin`).
- **Verify:** `cargo test offline lock privacy`; `npx vitest run src/lib/privacy.test.ts`; on a Mac: Settings →
  Privacy → Work offline, then ask "news today" (no search) and try a download (refused); turn on the lock (Touch ID
  asks), ⌘K → Lock BYTE now, unlock; open the Activity list.
- **Undo:** `git revert` this commit (the new settings are ignored by older builds).

## 2026-10-01

### (this commit) — v0.11.7: personality, help center, example prompts, brainstorm board (Phase 11 done)
- **Why:** the last Phase 11 items.
- **What:** `prompt.rs` (Personality + section + test), settings `personality`, `backend.rs` (personality local +
  cloud, help notes), `help.rs` (+ tests), `board.rs` (+ tests + `e2e_board_ideas`), `db.rs` migration v15, `lib.rs`;
  `src/help/*.md`; UI `lib/{help,examples,personality,board}.ts` (+ tests), `components/help/HelpCenter.tsx`,
  `components/board/BoardPanel.tsx`, `EmptyState` daily picks, Composer `prefill`, Shell (⌘?, buttons, palette),
  Settings `PersonalitySection`, tokens `--sticky-*`, CSS; screenshots `39-*` (the 06-chat scene clicks the first
  suggestion now that they rotate).
- **Verify:** `cargo test prompt help board db`; `npx vitest run src/lib/{help,personality,board}.test.ts`; real
  engine `cargo test e2e_board_ideas -- --ignored` (Qwen3 0.6B: 5 ideas, 2 themes); on a Mac: ⌘?, a personality
  preset then a question, the board's Add ideas / Group.
- **Undo:** `git revert` this commit (the boards table stays; harmless).

### (this commit) — v0.11.6: notes, web clipper, mind maps
- **Why:** Phase 11 items: notes (save answers, feed the knowledge base), a web clipper, mind maps from any answer.
- **What:** `src-tauri/src/notes.rs` (files, front matter, path safety, search, clip, commands, tests),
  `src-tauri/src/mindmap.rs` (Markdown → tree, model outline, clamp, tests + `e2e_mindmap_from_prose`),
  `background.rs` (`Link::Clip`, `CLIP_EVENT`), settings `notesEnabled`/`notesDir`, `lib.rs` commands; UI
  `components/notes/{NotesPanel,MindMapView}.tsx`, `lib/notes.ts`, `lib/mindmap.ts` (+ tests), store `notes`/`mindmap`,
  Shell button + palette, MessageView 📝 and mind-map buttons, Settings `NotesRow` (folder, bookmarklet), CSS,
  `--branch-*` tokens; screenshots `38-*`; `mac-engine.yml` path `mindmap.rs`.
- **Verify:** `cargo test notes mindmap background`; `npx vitest run src/lib/notes.test.ts src/lib/mindmap.test.ts`;
  real engine: `cargo test e2e_mindmap_from_prose -- --ignored` (passes on Qwen3 0.6B); on a Mac: 📝 under an answer,
  the Clip to BYTE bookmarklet on a page, 🧠 under an answer.
- **Undo:** `git revert` this commit (notes files stay on disk).

### (this commit) — v0.11.5: voice catalog, more human speech, cloud voice, "Hey BYTE" answers aloud
- **Why:** owner: voices as expressive and human as the big assistants; many voices from different free providers
  (no accounts) with descriptions; voice on the Mac or in the cloud; not taking memory from the chat model; and
  "Hey BYTE" working right after login.
- **What:** `scripts/build-voices.mjs` + `scripts/voices-cache.json` → `src-tauri/catalog/voices.json`;
  `src-tauri/src/voices.rs` (catalog, per-engine args, downloads/unpack, tests + `e2e_voices_speak`); `tts.rs`
  (packages, paragraph-aware chunks, pauses, fades, `Delivery` with cloud, new commands); `cloud/voice.rs` +
  `CloudClient::post_bytes`; `speech.rs` (`speakable` paragraphs + spoken steps, cloud choice, `speech_say` voice);
  `prompt::SPOKEN` + `ChatRequest.spoken`; settings `speechStyle`/`voiceWhere`/`cloudVoice`; UI `VoiceBrowser.tsx`,
  `lib/voices.ts` (+ test), `ByteVoicesRow`, `WakeRow`, Composer follow-up listening, `handsfree.isDone` (+ test),
  `public/voices/*.wav`; `mac-engine.yml` downloads four packages for the e2e; `CLUSTER-REQUESTS.md` request 7;
  screenshots `36-*`, `37-*`.
- **Verify:** `cargo test voices tts speech cloud::voice`; `npx vitest run src/lib/voices.test.ts src/lib/handsfree.test.ts`;
  locally every engine speaks through BYTE's code (`BYTE_TEST_SHERPA_TTS=<prebuilt> BYTE_TEST_VOICES=<dir> cargo
  test e2e_voices_speak -- --ignored`); on a Mac: Browse voices → download → ▶ → use; Talk mode; "Hey BYTE, …".
- **Undo:** `git revert` this commit (back to Kokoro only).

### (this commit) — “Hey BYTE”: quick phrases at the start of the audio were dropped
- **Why:** the Mac test (`e2e_wake_word`, run 36891745882) failed: "didn't hear Hey BYTE". `say` starts speaking at
  the first sample, so the voice detector had no pre-roll, yet subtracted a full 200 ms when measuring the burst; a
  ~0.5 s "Hey BYTE" measured under 0.4 s and was dropped. Users saying it briskly would hit the same thing.
- **What:** `src-tauri/src/wake.rs`: `Vad.pre` (the pre-roll actually kept), `MIN_BURST` 0.3 s, `padded()` (0.5 s of
  silence each side, at least 1.5 s, since whisper.cpp skips audio under a second), `heard_in_file` so the e2e test
  prints what whisper heard; tests `keeps_a_brisk_phrase_that_starts_the_audio`, `pads_short_bursts_for_whisper`.
  Docs: v0.11.3 ships inside v0.11.4.
- **Verify:** `cargo test wake`; locally Kokoro-spoken "Hey BYTE" (4 voices) → "Hey BYTE", and "Good morning", "Hey
  Bob", "Hey there", "Okay", "What time is it" → not a wake; Mac test `e2e_wake_word`.
- **Undo:** `git revert` this commit.

### (this commit) — v0.11.4: BYTE's own natural voices (Kokoro), smooth streaming speech
- **Why:** owner: the Mac's voice sounds robotic; wanted voices like the big assistants, several to choose from,
  smooth (no talk-pause-talk) and free.
- **What:** `src-tauri/src/tts.rs` (new: voices, download/unpack, sentences/chunks, sherpa-tts args, WAV read +
  resample, speaking queue + cpal output on macOS, `tts_*` commands, tests + `e2e_kokoro_speaks`), `speech.rs`
  (`speech_feed`, BYTE's voices first, `say` fallback), `models.rs` (`Downloads::start_from`), settings `byteVoice`,
  `lib.rs` wiring, `scripts/build-sherpa.sh` (TTS on, installs `sherpa-tts`), `tauri.conf.json` externalBin,
  `ci.yml`/`release.yml`/`mac-engine.yml`/`build-mac.sh`; UI `ByteVoicesRow`, store streams sentences to
  `speechFeed` while answering, api/types; screenshot `36-byte-voices`.
- **Verify:** `cargo test tts speech`; with sherpa-onnx's `sherpa-onnx-offline-tts` and the Kokoro folder:
  `BYTE_TEST_SHERPA_TTS=<bin> BYTE_TEST_KOKORO=<dir> cargo test e2e_kokoro -- --ignored`; on a Mac: download BYTE's
  voices, pick one, Try it, then Talk mode.
- **Undo:** `git revert` this commit (BYTE goes back to the Mac's voice).

### (this commit) — v0.11.3: spoken answers, hands-free conversation, “Hey BYTE”
- **Why:** Phase 11 items 1 (hands-free) and 4 (wake word); BYTE couldn't speak at all.
- **What:** `src-tauri/src/speech.rs`, `src-tauri/src/wake.rs` (+ tests and Mac e2e), `voice::transcribe_short`,
  `quick::show`, settings `readAloud`/`speechVoice`/`speechSpeed`/`wakeWord`, `lib.rs`/`commands.rs` wiring, `cpal`
  (macOS only) in `Cargo.toml`, `mac-engine.yml` paths; UI `lib/handsfree.ts` (+ test), store speaking/talk state,
  `MicButton` auto mode + wake pause, Composer Talk button and wake turn, `MessageView` 🔊, Esc in Shell/Quick Ask,
  Settings `SpeechRows`/`WakeRow`, CSS; screenshots `35-*` (the shots now use a Mac user agent).
- **Verify:** `cargo test speech wake`; `npx vitest run src/lib/handsfree.test.ts`; on a Mac: 🔊 on an answer, Talk
  mode, “Hey BYTE” with the setting on.
- **Undo:** `git revert` this commit.

### (this commit) — v0.11.2: speaker labels and videos without captions
- **Why:** Phase 11 items 1 and 1b (HANDOFF §8): transcripts with who said what; YouTube videos without captions.
- **What:** `src-tauri/src/speakers.rs` (+ tests, `e2e_speakers`, `e2e_labelled_transcript`), `src-tauri/src/media.rs`
  (+ tests), `voice.rs` (timestamped segments, `wav_16k`, speaker labels in `ingest`), `youtube.rs`
  (`spoken_transcript` fallback, `VideoCard.transcribed`), settings `voiceSpeakers`, `lib.rs` commands,
  `tauri.conf.json` `externalBin` + `binaries/sherpa-diarize`, `scripts/build-sherpa.sh`, `scripts/SHERPA_TAG`,
  workflows (build/cache sherpa in `release.yml` and `mac-engine.yml`, speaker test models, placeholders in `ci.yml`),
  `scripts/build-mac.sh`; UI `components/settings/VoiceExtras.tsx`, Voice section, `VideoCard.tsx`, `lib/video.ts`
  (+ test), Activity label, types/api; screenshots `34-*`; CLAUDE.md.
- **Verify:** `cargo test speakers media voice`; `scripts/build-sherpa.sh`, then the two speaker e2e tests with
  `BYTE_TEST_SHERPA`, `BYTE_TEST_SPEAKER_SEG`, `BYTE_TEST_SPEAKER_EMB`, `BYTE_TEST_SPEAKERS_AUDIO` (+ the whisper
  variables); on a Mac: attach a two-person recording; ask for a summary of a YouTube video without captions after
  downloading the video helper.
- **Undo:** `git revert` this commit (downloaded models stay in `<models>/voice/{seg,emb}`, yt-dlp in `<data>/tools/`).

### (this commit) — v0.11.1: voice input
- **Why:** Phase 11 item 1 (HANDOFF §8): talk instead of typing; recordings as transcripts.
- **What:** `src-tauri/src/voice.rs` (+ tests, real-engine `e2e_whisper_transcribes`), `scripts/build-whisper.sh`,
  `scripts/WHISPER_TAG`, `tauri.conf.json` `externalBin` + `binaries/whisper-cli`, `files.rs` (`FileKind::Audio`,
  `Ingested::transcript`), `commands.rs` `file_ingest` (audio → voice), `kb.rs` kind name, settings `voiceEnabled`/
  `voiceModel`/`voiceLanguage`, `tempfile` moved to dependencies; workflows (`release.yml`, `mac-engine.yml` build
  whisper-cli and run the voice e2e; `ci.yml` placeholder sidecars), `scripts/build-mac.sh`; UI `lib/wav.ts` (+ tests),
  `lib/recorder.ts`, `components/chat/MicButton.tsx`, `VoiceModels.tsx`, Composer (🎤, hold Space, audio in the file
  picker), Attachments (audio chip), Settings → Models → Voice, CSS; screenshots `33-*`; CLAUDE.md.
- **Verify:** `cargo test voice`; `scripts/build-whisper.sh` then `BYTE_TEST_WHISPER=src-tauri/binaries/whisper-cli-<triple>
  BYTE_TEST_WHISPER_MODEL=<ggml-base.en.bin> BYTE_TEST_WHISPER_AUDIO=.cache/whisper.cpp/src-v1.9.4/samples/jfk.wav
  cargo test e2e_whisper -- --ignored`; `npx vitest run src/lib/wav.test.ts`; on a Mac: 🎤 → download → talk → text.
- **Undo:** `git revert` this commit (voice models stay in `<models>/voice/`; delete the folder to free the space).

### (this commit) — v0.11.0: Quick Ask, menu bar, command palette, custom shortcuts
- **Why:** Phase 11 item 3 (HANDOFF §8): ask from any app, BYTE in the menu bar, ⌘K, customizable keys.
- **What:** `src-tauri/src/quick.rs` (+ tests), `icons/tray.png`, settings `quickAsk`/`quickAskKeys`/`selectionKeys`/
  `menuBarIcon`, `lib.rs` (shortcut handler, blur, tray at setup, commands), `commands.rs` `settings_update` (check +
  apply), `selection.rs` (`apply` moved to `quick::apply_shortcuts`), `capabilities/default.json` ("quick" window);
  UI `app/QuickAsk.tsx`, `app/App.tsx` (`useAppearance`, `windowKind`), `components/Palette.tsx`, `lib/palette.ts`,
  `lib/keys.ts` (+ tests), `components/settings/KeyboardSection.tsx`, store `addNewChats`/`openFresh`, Shell ⌘K,
  CSS; screenshots `32-*` (and a fixed `Connect` selector in `shots.mjs`); `mac-engine.yml` paths include `quick.rs`.
- **Verify:** `cargo test quick`; `npx vitest run src/lib/keys.test.ts src/lib/palette.test.ts`; `scripts/check-all.sh`;
  on a Mac: ⌥Space in another app, ask, Esc; click the menu-bar icon; ⌘K in BYTE; change the Quick Ask keys in
  Settings → About.
- **Undo:** `git revert` this commit (settings keep the new fields harmlessly; old builds ignore them).

### (this commit) — Connector secrets: remember Keychain reads, keep tests off the real Keychain
- **Why:** chat routing (`connectors::applies`) reads the Keychain on every message that mentions a calendar, "today"
  and the like; each read is a Security-framework call. And a test build on a CI Mac should never touch a real
  Keychain, which could wait on a prompt nobody answers.
- **What:** `src-tauri/src/connectors/mod.rs`: `Secrets` keeps what it read for the session (saving or removing
  updates it); the Keychain calls moved into a small `keychain` module, which in test builds (and off macOS) is a
  stub.
- **Verify:** `cargo test connectors`; on a Mac: connect Notion, ask "search Notion for …" twice (one Keychain read);
  disconnect, and the next ask no longer sees it.
- **Undo:** `git revert` this commit.

## 2026-09-30

### (this commit) — v0.10.6: dashboards (Phase 10 complete)
- **Why:** Phase 10 item 7 (HANDOFF §8): command-deck home, local usage stats, research library (the hardware panel
  already exists in Settings → Engine).
- **What:** `src-tauri/src/dashboard.rs` (+ tests on a temp DB) with commands `dashboard_summary`, `dashboard_today`,
  `dashboard_usage`, `research_library`; UI `components/chat/Deck.tsx` (tiles + research library) mounted in
  `EmptyState.tsx`, `lib/dashboard.ts` (+ tests), Settings → About "Your usage", CSS, screenshots `31-home-deck`,
  `31b-research-library`, `31c-usage`.
- **Verify:** `cargo test dashboard`; `npx vitest run src/lib/dashboard.test.ts`; `scripts/check-all.sh`; in the app: a new
  chat shows the tiles once there are to-dos, trackers, feeds or researched chats.
- **Undo:** `git revert` this commit (read-only feature; no data changes).

### (this commit) — v0.10.5: connectors (Obsidian, Notion, calendar links)
- **Why:** Phase 10 item 6 (HANDOFF §8). Owner decision: the connectors that need no app registration with another
  company come first; Google, Dropbox, OneDrive and Spotify wait for the owner to register BYTE with them.
- **What:** `src-tauri/src/connectors/{mod,obsidian,notion,ics}.rs` (+ tests: vault search/write, Notion mocked with
  wiremock, ICS fixtures with repeats/all-day/cancelled/EXDATE, request routing, secret store), settings
  `connectorsEnabled`/`obsidianVault`/`notionParent`, `Modules.connectors`, routing in `agent::specialist`, commands
  `connectors_status`, `obsidian_set`, `notion_connect`, `notion_disconnect`, `calendar_link_add`, `calendar_link_remove`,
  `briefing::merge_events`. UI: `ConnectorsTab.tsx`, types/API, Activity labels, screenshot `30-connectors`.
- **Verify:** `cargo test connectors briefing`; `scripts/check-all.sh`; on a Mac: Settings → Connectors → choose a vault
  → "what do my Obsidian notes say about …"; paste a Notion secret; add a Google Calendar iCal address → "brief me".
- **Undo:** `git revert` this commit (Keychain items under `com.loganstarner.byte.connectors` can be deleted in Keychain
  Access).

### (this commit) — v0.10.4: trackers
- **Why:** Phase 10 item 5 (HANDOFF §8): packages and orders, bills and subscriptions, gifts and events, car and home
  maintenance. Built while v0.10.3's Mac test ran (owner: "do stuff in parallel while we are waiting").
- **What:** `trackers.rs` + `trackers_tests.rs` (records, dates, carriers, money, notifications, chat), DB v14 `trackers`,
  `scheduler::tick` → `trackers::tick`, routing in `agent::specialist` after automations, canned `trackers_list` in
  `agent.rs`/`backend.rs`, setting `trackersEnabled` / `Modules.trackers`, commands `trackers_list`, `tracker_save`,
  `tracker_done`, `tracker_delete`, `tracker_date_parse`, `tracker_carrier`. UI `TrackersSection.tsx` in the ✅ panel,
  `lib/trackers.ts` (+ tests), Settings toggle, Activity labels, screenshots `29-trackers`, `29b-trackers-maintenance`,
  `29c-subscriptions`.
- **Verify:** `cargo test trackers`; `npx vitest run src/lib/trackers.test.ts`; `scripts/check-all.sh`; in the app: "add
  Netflix $15.49 a month on the 12th", "what subscriptions do I have?", "Sam's birthday is March 3", "change the furnace
  filter every 3 months", then "I changed the furnace filter today".
- **Undo:** `git revert` this commit (the `trackers` table stays and is unused).

### (this commit) — v0.10.3: automations, Shortcuts and opening at login
- **Why:** Phase 10 item 4 (HANDOFF §8): multi-step runs and an automations builder, plus creating Shortcuts (moved
  here from Phase 9) and opening at login (schedules only ran while BYTE was open). Owner: "keep going with v0.10.3".
- **What:** `automations.rs` (+ `automations_tests.rs`): steps, triggers, rule-based `plan` from chat, `execute` with
  resume, background runs with `automations://progress`, one chat per run, chat routing first in `agent::specialist`
  with an approval card. DB v13 (`automations`, `runs.automation_id`, `runs.steps`). `shortcut_make.rs` (binary plist
  via the `plist` crate, `shortcuts sign`, `open`). `background.rs` (`tauri-plugin-deep-link` `byte://run|ask`,
  `tauri-plugin-autostart` with `--background`, keep running on window close, Dock reopen). `web_agent::UNATTENDED`
  declines approvals at once in unattended runs (`scheduler::ask_unattended`). `tauri.conf.json`: main window starts
  hidden (shown in setup unless `--background`), deep-link scheme `byte`. Settings `automationsEnabled`,
  `openAtLogin`, `keepRunning`. UI: `AutomationsSection.tsx` in the ✅ panel, `RunCard.tsx`, `lib/automations.ts`
  (+ tests), Settings → About "In the background" and Features toggle, composer fills from `deeplink://ask`,
  Activity label. `mac-engine.yml` runs on the new files (plutil + `shortcuts sign` check, the macOS-only Reopen code).
  Screenshots `28-automations`, `28b-automation-approval`, `28c-automation-run`.
- **Verify:** `cargo test automations background shortcut_make`; `npx vitest run src/lib/automations.test.ts`;
  `scripts/check-all.sh`; on a Mac: ask "research X, then write a short guide from it and save it to a file" → Do it →
  the card fills in, a notification, the file in Documents/BYTE/Automations; ✅ → Automations → link button → Add
  Shortcut → run it from Shortcuts; Settings → About → Open BYTE at login, log out and in.
- **Undo:** `git revert` this commit (DB v13 stays; older builds ignore the new table and columns).

### (this commit) — Mac test fixes: split terminal commands, a steady thinking-off check
- **Why:** the Mac engine run on v0.10.1 failed 2 of 35 tests, both on Qwen3 0.6B. (1) `terminal::e2e_terminal_proposals`:
  the model put a line break before a pipe (`find … {} \n| grep -c 'file'`), and `parse` refuses any multi-line
  command, so `propose` returned nothing (reproduced here about 1 run in 8). (2) `chat::e2e_streams_from_real_llama_server`:
  with thinking off the model answered "12 + 30" with "15". That check is about streaming, and in the app sums go to
  the calculator.
- **What:** `terminal.rs`: `joined()` turns a command split only where the shell carries on (a line ending in `\`,
  `|`, `&&`, `||`, or the next line starting with a pipe or `&&`/`||`) into one line; any other break is still refused
  (it would be a second command). `propose` asks once more when a reply can't be read. Unit test
  `line_breaks_only_where_the_shell_carries_on` (the real CI reply included). `chat.rs`: the thinking-off check copies
  one word ("pineapple") instead of adding numbers.
- **Verify:** `cargo test terminal`; real engine: terminal e2e 12/12, streaming e2e 5/5 on Qwen3 0.6B.
- **Undo:** `git revert` this commit.

### (this commit) — v0.10.2: news feeds and page watchers
- **Why:** Phase 10 item 3 (HANDOFF §8): news/RSS digest and page watchers with price alerts, built while the v0.10.0
  release and the v0.10.1 Mac run ran.
- **What:** `db.rs` v12 (`feeds`, `feed_items`, `watchers`, `watch_events`). `feeds.rs` (RSS/Atom parser on
  `quick-xml`, discovery, storing, digest composed by BYTE, chat routing, commands). `watchers.rs` (content-line
  diff, price from JSON-LD → microdata → Amazon's price-to-pay element, `price_change`, due checks from the scheduler
  tick, chat routing with approval card, commands). `tools/fetch.rs` `fetch_raw` + feed types in `Accept`.
  `scheduler::tick` calls `watchers::tick`; `agent`/`backend` send `feeds_digest` as BYTE's own reply; setting
  `watchEnabled`, `Modules.watch`. UI: `WatchSection.tsx` in the ✅ panel, `lib/watch.ts` (+ tests), Activity labels,
  Settings toggle, `.schedule-list .danger` (error hints were not red), screenshot `27-feeds-watch`.
  Fix: `briefing::write` checked `web_mode != "off"` (always true) instead of `web_search`.
- **Verify:** `cargo test feeds:: watchers::`; `cargo test e2e_real -- --ignored --nocapture` (live: HN, Rust blog,
  The Verge found from its homepage, BBC, GitHub blog; the run caught double-escaped entities in The Verge's titles,
  fixed). Amazon serves this environment a Captcha, so its price element is tested on a snippet only. Owner: "follow
  theverge.com" → "what's new in my feeds?"; "tell me when <product link> drops below $X" → Do it → ✅ panel.
- **Undo:** revert; Settings → News feeds and watched pages off stops all checks (the tables stay, unused).

### (this commit) — v0.10.1: tasks, schedules and a daily briefing
- **Why:** Phase 10 item 2 (HANDOFF §8): tasks & reminders, a scheduler, the daily briefing. Built while the v0.10.0
  Mac run and release ran.
- **What:** `db.rs` v11; `scheduler.rs` (specs, `next_after`, `spec_in`, schedules/runs, unattended runs, notifications,
  loop); `tasks.rs` (to-dos, reminders, repeats, chat routing, scheduled questions after approval); `briefing.rs`
  (gathers calendar/reminders via `macctl::read_notes`, to-dos, weather, followed news; composes the Markdown itself;
  `agent`/`backend` send it as BYTE's own reply). `tauri-plugin-notification`. Settings `tasksEnabled`,
  `briefingTopics`; `Modules.tasks`. UI: `TasksPanel.tsx` (✅), `lib/tasks.ts` (+ tests), Activity labels, Settings,
  screenshots `26-tasks`, `26b-briefing`, `26c-schedule-approval`. Docs and version 0.10.1.
- **Verify:** `cargo test scheduler:: tasks:: briefing::`; `npx vitest run src/lib/tasks.test.ts`. A model-written
  briefing was tried first on Qwen3 0.6B, Gemma 3 1B and Llama 3.2 1B; Gemma invented a to-do and news, so BYTE now
  composes it. Owner: "brief me"; "add pay rent by Friday to my to-do list"; "every weekday at 8am, summarize the
  latest AI news" → Do it → ✅ → Run now.
- **Undo:** revert; Settings → To-do list and schedules off disables it (the DB tables stay, unused).

### (this commit) — Translation: low temperature, so small models keep the right words
- **Why:** the Mac engine run on `7eb336b` (v0.10.0) passed every new upkeep test (all scripts compile, the health
  check runs on a real Mac) but failed `translate::e2e_translate`: Qwen3 0.6B wrote "Libro abrige…" ("book") for
  "The library opens…". Translation used the model's chat sampling (temperature 0.7 for Qwen3).
- **What:** `translate.rs`: `TEMPERATURE = 0.2` for every translated part (like the writing studio's grammar fix at
  0.1); a unit test pins it low. `mac-engine.yml` now also runs on `translate.rs` changes.
- **Verify:** `e2e_translate` on Qwen3 0.6B locally 4/4 with "biblioteca" (before: failed on the Mac runner).
- **Undo:** revert.

### (this commit) — v0.10.0: Mac upkeep (Phase 10 starts)
- **Why:** Phase 10 item 1 (HANDOFF §8): storage analyzer, battery & performance coach, self-diagnostics, app
  uninstaller, login items. Built while the v0.9.3 release built (v0.9.3 is published with its .dmg).
- **What:** `upkeep.rs` + `upkeep_tests.rs` (routing, scan, duplicates, suggestions, Trash/Put Back via Finder with
  Undo, health checks from read-only system tools, uninstaller with Library leftovers, login items); commands
  `upkeep_trash/reveal/quit/open_settings` take ids from BYTE's own latest card, never paths. `ChatEvent::Storage`
  and `Health`; routed in `agent::specialist` before the terminal helper; `Modules.upkeep` / setting `macUpkeep`.
  `macctl.rs`: tool paths for the new read-only programs, `pane_label`/`settings_url` shared; the Mac compile test
  now compiles every module's scripts (macctl, selection, files, upkeep). UI: `UpkeepCards.tsx` (treemap from
  `lib/upkeep.ts`, confirm + Undo, health list, Quit), Settings toggle, screenshots `24-storage`–`24c-health`.
  Docs: release notes, VERSIONS, CHANGELOG, HANDOFF, PROJECT_GUIDE. Version 0.10.0.
- **Verify:** `cargo test upkeep` (20 tests), `npx vitest run src/lib/upkeep.test.ts`; Mac CI:
  `e2e_scripts_compile_and_run` (osacompile of the new scripts) and `e2e_health_on_a_real_mac`. Owner: "what's
  taking up space?" → Move to Trash → Undo; "why is my Mac slow?"; "uninstall <some app>" → Don't.
- **Undo:** revert; Settings → Mac upkeep off disables it.

## 2026-09-29

### (this commit) — Terminal helper: known-good commands; small models don't invent commands
- **Why:** the Mac engine run on `22c49ef` failed `e2e_terminal_proposals`: Qwen3 0.6B on Metal answered "what's
  using port 3000" with `ps aux | grep 3000`. Local runs showed worse (asked to count files, it printed every file).
- **What:** `terminal::recipe` (ports, disk, folder sizes, biggest files, IP, battery, uptime, CPU/memory, macOS
  version, hardware, listing a folder, date, running apps; only digits and fixed folder names reach the command);
  small models (≤ 4B) run only recipes and otherwise explain the steps; bigger models' commands get a "Check" line
  on the approval card; prompt no longer lists example commands (small models copied them).
- **Verify:** `cargo test terminal::` (recipes, `small_models_only_run_known_commands`); the e2e now checks the
  model path's plumbing on Qwen3 0.6B / Llama 3.2 1B / Qwen3.5 2B.
- **Undo:** revert.

### (this commit) — v0.9.3: files and a terminal helper (Phase 9 complete)
- **Why:** the rest of Phase 9 (HANDOFF §8), built while the v0.9.2 Mac test ran.
- **What:** `filectl.rs` (+ `filectl_tests.rs`): find (mdfind), tidy by kind/month with preview + undo, convert/shrink
  the Finder selection with sips (originals kept, undo removes copies), read the Finder selection. `terminal.rs`:
  one proposed command, hard refusals, approval, run with zsh, output explained. `macctl.rs`: `Undo` enum,
  `keep_undo`, `move_back`, `ask_ok`; `MacRunner` knows mdfind/sips/zsh. Routed in `agent::specialist` after
  macctl. UI: monospace Command field; Settings text; screenshot `23h-terminal`. Mac CI covers the new files.
- **Verify:** `cargo test ctl terminal::`; real models (Qwen3 0.6B, Gemma 3 1B, Llama 3.2 1B) propose `df -h` and
  `lsof -i :3000` (`e2e_terminal_proposals`); owner: "organize my Downloads" → Do it → Undo.
- **Undo:** revert; Settings → Mac control off disables all of it.

### (this commit) — Mail check: a variable named after an AppleScript word
- **Why:** the Mac engine run on `e450631` failed `e2e_scripts_compile_and_run`: `MAIL_LIST` used a variable named
  `since`, which AppleScript reads as a parameter name, so "check my email" would fail on every Mac. Caught before
  release.
- **What:** `macctl.rs` `since` → `cutoff`; the macOS compile test now reports every failing script, not just the
  first; `scripts_avoid_applescript_keywords_as_variables` checks all fixed scripts for reserved words on any OS.
- **Verify:** `cargo test macctl`; Mac engine run on the new head (osacompile of all scripts).
- **Undo:** revert.

### (this commit) — v0.9.2: selected text anywhere (⌥⌘B) and clipboard history
- **Why:** Phase 9's selection tools and clipboard history (HANDOFF §8), built during the v0.9.1 Mac CI wait.
- **What:** `selection.rs`, `clipboard.rs` (+ DB v10), writing `Reply`/`Explain` (`HELPER`, `echoes`, `reply_text`),
  settings `selection_hotkey` (on) / `clipboard_history` (off), `settings_update` re-applies the hotkey; deps
  `tauri-plugin-global-shortcut`, objc2-app-kit `NSPasteboard`. UI: `ClipboardPanel.tsx`, `lib/clips.ts` (+ test),
  studio Reply/Explain + "Paste into <app>", Shell listens for `selection://*`, Settings toggles; screenshots `23f`,
  `23g`. Mac CI covers `selection.rs`/`clipboard.rs` (`e2e_selection_scripts_and_clipboard`).
- **Verify:** `cargo test clipboard selection writing::`; real models: reply e2e on Qwen3 0.6B (echo caught, retried),
  Llama 3.2 1B, Qwen3.5 2B (good); Gemma 3 1B still under-shortens (known, small model). Owner: select text in Mail,
  ⌥⌘B, Reply, Paste into Mail.
- **Undo:** revert; both features can be switched off in Settings.

### (this commit) — v0.9.1: Mail and Messages
- **Why:** next Phase 9 slice (plan: Mail + Messages drafts), same safety rules: fixed scripts, words as argv,
  approval, never send.
- **What:** `macctl.rs`: `MailList`/`MailDraft`/`MessageDraft`/`ContactFind`, scripts `MAIL_LIST`, `MAIL_DRAFT`,
  `MAIL_DRAFT_DELETE`, `MESSAGE_DRAFT`, `CONTACT_FIND`; routing (clear requests only; "write an email to…" stays a
  chat answer); `find_person`, `sender_parts`, `sms_url`, `without_placeholders`; `details` takes the runner for
  lookups. Approval fields keep line breaks (`app.css`). Settings text; screenshots `23d-mail-draft`, `23e`.
- **Verify:** `cargo test macctl` (21); real models: `BYTE_TEST_MODEL=<Qwen3-0.6B|gemma-3-1b|Llama-3.2-1B> cargo test
  macctl -- --include-ignored` (all pass, drafts without "[Your Name]"); Mac CI compiles the new scripts.
- **Undo:** revert.

### (this commit) — Writing studio: Shorten and Expand get a word target
- **Why:** the Mac engine test failed on v0.8.0 (`writing::tests::e2e_writing`): asked for "about half", Qwen3 0.6B
  cut only 15%. A real quality bug on small models, not a flake.
- **What:** `writing::length_target` adds "The text has N words; your version must have at most N/2 words" (Expand:
  about 2×N) to the request; unit test `shorten_and_expand_get_a_word_target`.
- **Verify:** `BYTE_TEST_MODEL=Qwen3-0.6B cargo test writing::tests::e2e_writing -- --ignored` (3/3 pass; 357 → 132
  characters).
- **Undo:** revert.

### (this commit) — v0.9.0: Mac control, part 1
- **Why:** Phase 9 (owner: keep going through the phases). First slice: the Apple apps people use most, done safely.
- **What:** `macctl.rs` (+ `macctl_tests.rs`): fixed AppleScripts with the user's words only as `argv`, rules first
  (`plan`, `when_in`, `reminder_title`) and `complete_json` for details, approval card (`web_agent::ask` now
  `pub(crate)`, action "mac") for lasting changes, `ChatEvent::MacDone` + `mac_undo`, `settings.mac_control`;
  routed first in `agent::specialist`. UI: `MacCard.tsx`, approval card for "mac", Activity labels, Settings toggle,
  screenshots `23*`. `mac-engine.yml` runs `e2e_scripts_compile_and_run` (osacompile every script). Docs + bump 0.9.0.
- **Verify:** `cargo test macctl` (14); real models: `BYTE_TEST_MODEL=<Qwen3-0.6B|gemma-3-1b|Llama-3.2-1B> cargo test
  macctl -- --include-ignored` (all pass); the owner tries "remind me to … tomorrow at 3pm" on the Mac.
- **Undo:** revert; the module can also be switched off in Settings.

### (this commit) — Version 0.8.0 (Phase 8 complete)
- **What:** `docs/releases/v0.8.0.md`, VERSIONS, CHANGELOG, HANDOFF, PROJECT_GUIDE; `node scripts/bump.mjs 0.8.0`.
- **Verify:** `scripts/check-all.sh`; the `release.yml` run for `v0.8.0` attaches the .dmg.
- **Undo:** revert (the release stays on GitHub until deleted).

### (this commit) — Phase 8 UI review: real recommended values, error banners
- **Why:** reviewing the helper's panels to the same bar as the rest (owner: "same quality work"). The sliders sat
  at a fixed 0.7 / 0.8 while "Recommended"; four of my panels used a `banner error` class that doesn't exist.
- **What:** `commands::LiveStats.recommended` (the loaded model's plain sampling from `modelcfg::profile`) →
  `types.ts`; `TuningPanel` polls once (`useLive`) and shows "Recommended (0.60)" at that position; wider
  `.tuning-value`; `banner danger` in Jobs/Assistants/Longform/WritingPanel; shots mock.
- **Verify:** `npm run typecheck && npx vitest run`; screenshots `07j-model-lab`, `07k-tuning`; `cargo check`.
- **Undo:** revert.

### (this commit) — Phase 8 UI, first pass (Model lab + Advanced tuning panels, in progress)
- **Why:** a snapshot of the parallel UI work, so it's saved while it continues. Typecheck and vitest pass (138).
- **What:** `components/settings/{ModelLab,TuningPanel}.tsx`, `lib/tuning.ts` (+ tests), Settings mounts, CSS,
  screenshot mocks. They're built on the contract in `types.ts`/`api.ts`.
- **Verify:** `npm run typecheck && npx vitest run`. It's reviewed with screenshots in the next commit.
- **Undo:** revert.

### (this commit) — Custom assistants in Cloud mode
- **Why:** v0.7.6 said the cloud didn't get an assistant's instructions. Built while the helper wrote the Phase 8 UI.
- **What:** `assistants::cloud_message` puts the instructions under the user's words (the cloud chat API has no
  system prompt); `backend::answer`'s cloud branch uses it for chats with an assistant.
- **Verify:** `cargo test assistants::` (`cloud_messages_carry_the_instructions`).
- **Undo:** revert.

### (this commit) — Phase 8 backend: model lab, advanced tuning, battery saver, live meters
- **Why:** these are Phase 8's remaining items. The owner asked for work in parallel: this is the Rust half plus the
  TS contract (`types.ts`/`api.ts`); a helper builds the UI panels at the same time in separate files (next commit).
- **What:**
  - `gguf.rs`: reads a GGUF header only (8 MB, then up to 64 MB): architecture, name, quant (file_type), params
    (size_label or estimated), thinking template, and `ModelArch` like build-catalog.mjs `archFrom`, including
    hybrid KV layers.
  - `lab.rs`:
    - `lab_inspect` (a file) and `lab_inspect_url` (Hugging Face: size and SHA-256 from the redirect headers,
      header by range request) → `LabModel`, with fit from `models::plan`;
    - `lab_add` links a file into the models folder (or keeps the Hugging Face repo/file for the normal
      downloader); `lab_list`/`lab_remove`; saved in `<data>/added_models.json`;
    - `CatalogStore::set_added` merges added models into `get()` (tag "added", never recommended), loaded at
      startup.
  - `settings.model_overrides` (`ModelOverride::apply`: temperature, top-p, thinking budget, clamped) plus
    `system_extra` appended to the system prompt, applied in `backend::Setup`.
  - `settings.battery_saver` + `system::battery`/`parse_pmset`/`low_battery`: under 20% and unplugged, Deep and
    Extended run as Auto and thinking is capped at 512, with a notice.
  - `commands::engine_live` (RAM, the engine's RSS by PID, GPU budget, battery, saving).
- **Verify:**
  - `cargo test` (294);
  - real files: `e2e_reads_a_real_gguf` (Qwen3-0.6B Q8_0: qwen3 / 28 layers / 40,960 context / thinks; Gemma 3 1B:
    gemma3 / 26 layers / 1 KV head / 256 head dimension);
  - live: `live_hf_header` (unsloth/Qwen3-0.6B-GGUF Q4_K_M in 1.7 s).
- **Undo:** revert (added_models.json is then ignored).

### (this commit) — v0.7.7: long-form writer, "Write like me", poems and speeches (Phase 7 done)
- **Why:** these are the last Phase 7 items; the owner said "Lets start back up".
- **What:**
  - `writing.rs`:
    - `stream_text` is shared by all studio streams (local model, or a cloud helper conversation);
    - `writing_outline` (`complete_json`: title + 2–10 sections sized to the length) and `writing_section`, which
      streams one part with the plan, the last ~1,500 characters so far, and the part's points. BYTE writes the
      `#` title and `##` headings itself, since Qwen 0.6B garbled them as "## # …";
    - poems in one pass, with forms (haiku, sonnet, limerick, rhyming, free verse); speeches at 130 words a minute
      with a call to action at the end;
    - `style_learn` turns 1–3 samples into a short profile saved in `settings.writing_style`, drops lines that copy
      6+ words of a sample (Qwen 0.6B did that), and retries once. `style_rules` adds it to the studio's Edit actions
      (not Fix grammar) and to long pieces when "Write like me" is on.
  - UI:
    - `components/writing/Longform.tsx`: the kind / topic / length form → an editable plan (rename, reorder,
      remove, add) → part-by-part writing → Save (.md) / Copy / Edit it.
    - `StyleSetup` ("Teach BYTE your style", edit or forget it); tabs in `WritingPanel`; the "Write like me"
      switch.
- **Verify:**
  - `cargo test writing::`; `npx vitest run src/lib/writing.test.ts`;
  - real engine `e2e_longform_and_style` on Qwen3-0.6B and Gemma 3 1B;
  - screenshots `23b-writing-new`, `23c-writing-outline`, `23d-writing-longform`.
- **Undo:** revert.

### (this commit) — v0.7.6: custom assistants
- **Why:** custom assistants are the next Phase 7 item. Built while the v0.7.5 release was building.
- **What:**
  - `assistants.rs` + DB schema v9 (`assistants`, and `conversations.assistant_id`, kept on save like `cloud_id`):
    name, emoji, instructions, up to 4 starters, a default mode; four presets (Email helper, Study coach, Coding
    buddy, Fitness planner).
  - `ChatRequest.assistant_id` → `backend::Setup` adds `prompt_section` ("you are the user's '<name>' assistant
    (you're still BYTE)" + the instructions).
  - UI: `components/assistants/AssistantsPanel.tsx` (list, edit, presets, Chat), a 🤖 top-bar button, store
    `newChat(…, assistant)` (sets the assistant's mode), `EmptyState` shows the assistant and its starters, and
    Settings → Features `assistantsEnabled`.
  - Not yet: assistants in Cloud mode (the cloud chat gets the user's words only).
- **Verify:** `cargo test assistants:: db::`; typecheck; screenshots `25-assistants`, `25b-assistant-chat`.
- **Undo:** revert (the v9 table and column stay, unused).

### (this commit) — v0.7.5 (part): job search tracker
- **Why:** the job tracker is the next Phase 7 item. Built while the v0.7.4 release was building.
- **What:**
  - `jobs.rs` + DB schema v8 (`jobs`): list sorted by the soonest deadline, save (moving past "saved" stamps the
    applied date), delete.
  - `job_from_url` reads a posting: `fetch_page`, then `read_posting` (`complete_json`, or the cloud with no local
    model).
  - `deadline_in` backstops a deadline the model missed ("Apply by…", "Applications close November 5, 2026",
    "Deadline: 15 Nov 2026", 12/01/2026).
  - `prep_prompt` gives the "Prepare for the interview" chat message.
  - UI: `components/jobs/JobsPanel.tsx` (paste a link → a filled-in job to review; grouped by status; deadline
    countdown; open / prep / delete), `lib/jobs.ts` (+ tests), a 💼 top-bar button, and Settings → Features
    `jobsEnabled`.
- **Verify:**
  - `cargo test jobs::`; `npx vitest run src/lib/jobs.test.ts`;
  - real engine `e2e_read_posting`: Qwen3-0.6B is fully right; Llama 3.2 1B missed the deadline until the
    `deadline_in` backstop, and now passes;
  - screenshot `24-jobs`.
- **Undo:** revert (the v8 table stays, unused).

### (this commit) — v0.7.5 (part): translate
- **Why:** it's the next Phase 7 item. Built while the v0.7.4 release was building.
- **What:**
  - New `translate.rs`:
    - `ask()` finds "translate … into/to/in <language>" (60+ languages, the last one named wins);
    - the text comes from a quote or a colon in the message, a link (Web on), the attached file (`<file>` blocks), or
      the answer above;
    - `parts()` splits the text between paragraphs or sentences (1,800 characters);
    - `run()` translates part by part with a strict translator prompt and streams each part.
  - `agent::run` tries it before the card flows (`Modules::translate`, `settings.translate_enabled`) and ends the turn
    itself.
  - The writing studio gains Translate with a language list (`Action::Translate`).
  - Activity label; Settings → Features toggle.
- **Verify:**
  - `cargo test translate::`;
  - real engine `e2e_translate` ("Translate that into Spanish" after a two-paragraph answer): Gemma 3 1B is perfect;
    Qwen3-0.6B works but its Spanish has mistakes (the test checks the flow, not a 0.6B model's grammar).
- **Undo:** revert.

### (this commit) — v0.7.4: the writing studio
- **Why:** it's the next Phase 7 item (VERSIONS). Built while the v0.7.3 release was building.
- **What:**
  - `writing.rs`: `Action` (rewrite / expand / shorten / tone / grammar), `instructions`, `max_tokens` per action,
    `request_body` (thinking off; grammar at temperature 0.1), and the `writing_run` command. It streams `Content`
    through `chat::stream_round`, is cancellable with `chat_cancel`, and with no local model uses the cloud
    (`JsonHelper::text`, a helper conversation that's deleted).
  - UI:
    - `components/writing/WritingPanel.tsx`: your text on the left, the new version on the right with changed words
      highlighted; a selection changes only that part; Use this / Copy / Discard / Stop.
    - `lib/writing.ts` (+ tests): word-level LCS `changes`, `cleanResult` (drops "Here is…", fences, quotes),
      `target`/`applyResult`.
    - A ✍️ top-bar button, "Edit in the writing studio" on answers, and Settings → Features `writingEnabled`.
- **Verify:**
  - `cargo test writing::`; `npx vitest run src/lib/writing.test.ts`;
  - real engine: `e2e_writing` (shorten is shorter and keeps "Thursday"; grammar fixes "libary"/"tomorow") passes
    with Qwen3-0.6B and Llama 3.2 1B. Neither small model fixes "Their going" (a limit of 1B models);
  - screenshot `23-writing-studio`.
- **Undo:** revert.

### (this commit) — "A better model fits your Mac" hint (once per small model)
- **Why:** it was part of the owner's "make all models do good" request, and left out of v0.7.3. Built while the
  v0.7.3 release was building (new way of working: code during waits).
- **What:**
  - `models::better_model`: the recommended model, if the one in use is ≤ 4B and the recommendation is at least
    twice its size and fits comfortably.
  - `backend::better_model_hint` sends one `ChatEvent::Notice` at the start of a local turn, remembered in
    `settings.better_model_hint_for`.
- **Verify:** `cargo test models::` (16 GB Mac: a hint for Qwen3-0.6B, none for the recommended model).
- **Undo:** revert.

### (this commit) — v0.7.3: the photo helper for models that can't see
- **Why:** the owner asked for every model to "do good". Until now, a photo sent to a model without vision got
  only its OCR text and a "can't see images" note.
- **What:**
  - New `looker.rs` (like `embed.rs`): a second on-demand `Engine::helper` runs a small vision model from the catalog
    with its image adapter; it prefers the default `qwen3.5-0.8b` (≈740 MB with its adapter) and stops after 5 idle
    minutes. `describe()` makes one request with anti-repeat sampling, since the 0.8B looped without it.
  - `backend::describe_photos`: when the model can't see and `settings.photo_helper` is on, the latest photos (up to
    4) are described first. The text starts with `looker::DESCRIBED`, and a "look_at_photo" activity step is shown.
  - `chat::with_files` tells the model the description came from the helper.
  - `commands::looker_status`; Settings → About → Features "Photo helper" row (toggle + Download); Activity label.
- **Verify:**
  - `cargo test looker::`;
  - real engine: `BYTE_TEST_VISION_MODEL=Qwen3.5-0.8B-Q4_K_M.gguf BYTE_TEST_VISION_MMPROJ=mmproj-F16.gguf cargo
    test e2e_looker -- --ignored` (fixture `tests/fixtures/photo_red_square.png`), 3/3: "red" and "BYTE 1234";
  - screenshot `07i-settings-photo-helper`.
- **Undo:** revert (photos go back to OCR text only for non-vision models).

### (this commit) — Card quality checks and one repair for small models (v0.7.3, part 1)
- **Why:** the owner asked to "make all models do good". 1B models pass the card tests, but their cards can be poor
  (six fronts saying "Photosynthesis", a "Flat White Lentil Soup").
- **What:**
  - New `quality.rs`:
    - checkers for flashcards, quizzes, recipes (wrong dish, including a title that adds a dish kind like "soup"
      that wasn't asked for; amounts; steps), recipe ideas, meal plans, compare scores and trips;
    - `improve()` makes one repair call listing the problems and keeps the repair only if it has fewer.
  - Wired after each card's JSON call in `study.rs`, `kitchen.rs`, `decide.rs` and `trip.rs`.
  - Runs only for small models (`Modules::small_model`: ≤ 4B or not in the catalog; set in `backend::Setup`), never
    for cloud JSON; a failed repair keeps the first card.
- **Verify:**
  - `cargo test quality::`;
  - real engine `e2e_kitchen e2e_study` with `small_model` on: Qwen3-0.6B and Llama 3.2 1B both pass, with repairs
    tried for a generic front and too-short backs.
- **Undo:** revert (cards are then shown as the model wrote them).

### (this commit) — Retry JSON cut off by reasoning; four model families pass the card tests
- **Why:** the third pass left 2 DeepSeek-R1 failures (recipe ideas, meal plan). It reasoned (~600 tokens) and then
  its JSON was cut off at `max_tokens`. `complete_json` only retried when the reply was empty.
- **What:** `chat::complete_json` also retries with room when the reply finished with `length` after reasoning.
- **Verify:** the card e2e set (study, kitchen, compare, fact-check, trip, summarize, calculator, web, reviews/prices,
  prepare_for_cloud, docs): Qwen3-0.6B 11/11, Gemma 3 1B 11/11, Llama 3.2 1B 11/11, and DeepSeek-R1-Distill 1.5B 9/11
  before this fix, with the 2 failures (kitchen, prepare_for_cloud) passing after it.
- **Undo:** revert.

### (this commit) — docs/CLUSTER-REQUESTS.md: what the app needs from the cluster
- **Why:** the owner asked for a doc in the repo telling the cluster what the app needs from it as features start
  leaning on it (like card making). It also reminds the cluster that this repo is public, so it must be secure.
- **What:**
  - New `docs/CLUSTER-REQUESTS.md`:
    - the public-repo rules (no keys, internal addresses, topology, user data or admin endpoints; every new
      endpoint needs auth, per-account scoping, rate and size limits, untrusted input, plain errors);
    - six requests, each with why, a suggested shape and the app's fallback: `POST /api/complete`, hidden
      conversations, confirming `DELETE /api/conversations/{id}`, a `card` stream event, a helper budget, and
      `POST /api/vision/describe`;
    - how the app behaves toward the cluster.
  - Linked from `CLAUDE.md` and `docs/CLOUD-MODE.md`.
- **Verify:** `scripts/check-secrets.sh`; read the file (no hostnames beyond the public base URL).
- **Undo:** revert (docs only).

### (this commit) — Models that reason when told not to; no example in the title prompt
- **Why:** the second pass of the per-family e2e tests.
  - DeepSeek-R1-Distill 1.5B still failed the calculator, recipe and document-section tests. Its extra
    `complete_json` room overflowed the 4k test context, and chat rounds had no room at all, so it reasoned until
    `max_tokens` and answered "**".
  - Qwen3-0.6B sometimes copied the example title from the chat-title prompt, now that the format is enforced by a
    schema.
- **What:**
  - `chat.rs`:
    - `with_room` fits the extra room into what the context has left.
    - `cap_reasoning` sets `reasoning_budget_tokens` (1,536) when a model reasons unasked.
    - `stream_round` learns that quirk from any round that reasoned without being asked (thinking not planned). It
      retries a round that ran out before answering, and gives later rounds room plus the cap (`for_reasoner`).
  - `summarize.rs`: the prompt has no example (the schema fixes the shape).
- **Verify:**
  - `cargo test chat::` (`reasoners_get_room_within_the_context`);
  - `e2e_summarizes_a_chat` passed 12/12 with 0.6B;
  - the family e2e set on R1, Qwen 0.6B, Gemma 3 1B and Llama 3.2 1B.
- **Undo:** revert.

### (this commit) — Cards in Cloud mode with no model on this Mac
- **Why:** the owner said yes to cards in Cloud mode even with no model loaded here, accepting that the helper
  requests may show in the cloud's chat list. Until now, `prepare_for_cloud` needed a local engine.
- **What:**
  - `cloud/json.rs` (new): `JsonHelper::complete` opens a "BYTE card helper" conversation, posts the task plus
    "reply with only JSON matching this schema", follows the answer quietly, then deletes the conversation (if the
    server allows). `pick_mode` prefers Auto, then Fast.
  - `engine::Endpoint.cloud`: when set, `chat::complete_json` asks the cloud. Every card flow uses only
    `complete_json`, so none of them changed.
  - `backend::cloud_cards` builds that endpoint; `prepare_for_cloud` uses it when no engine is loaded.
  - `summarize::short_title`: titles are at most 8 words and don't end on "a"/"for"… (Llama wrote 9-word titles).
- **Verify:** `cargo test cloud::` (`cards_come_from_the_cloud_when_no_model_is_loaded_here`: helper conversation
  created, schema in the prompt, fenced JSON parsed into a flashcards card, the conversation deleted, no JSON shown).
  With a real cloud: owner, in Cloud mode with no model loaded, "Make 5 flashcards about tides".
- **Undo:** revert; Cloud mode without a local model then answers with text only again.

### (this commit) — Cards and tools on Gemma, Llama and reasoning models
- **Why:** the owner asked whether all features work with all models. The card e2e tests had only run on Qwen. With
  Llama 3.2 1B, Gemma 3 1B and DeepSeek-R1-Distill 1.5B:
  - Gemma's chat template rejects tool messages ("roles must alternate"), so every forced tool turn failed with a
    400 (the calculator test; also the kitchen test).
  - Llama wrote a tool call the engine couldn't parse ("does not match the expected peg-native format").
  - R1 ignores `enable_thinking: false` and used up `max_tokens` on reasoning, so `complete_json` (recipe ideas,
    document sections) and the chat title came back empty.
- **What:** `chat.rs`:
  - `stream_round` retries once with `plain_body` (no tools; tool calls and results become plain text; same-speaker
    turns merged) when the engine reports a template or tool-call parse error. It remembers the model (`Quirks.plain`).
  - A tool-call parse error after the answer has started keeps the answer.
  - `complete_json` gives models that reason first 3,072 more tokens (retry once, then `Quirks.reasons`).
  - `summarize.rs` now goes through `complete_json` with a schema.
- **Verify:**
  - `cargo test plain_body_turns_tool_calls_into_text`;
  - the card e2e tests with each of the four GGUFs (`BYTE_TEST_MODEL=...`): study, kitchen, compare, fact_check,
    trip, summarizes, calculator, web, reviews, prepare_for_cloud, docs.
- **Undo:** revert (models with those templates then fail again).

### (this commit) — Flashcards from small models: schemas in written order, one retry, cut-off replies kept
- **Why:** the Mac engine test on `c9bb9c0` failed in `study::tests::e2e_study`, and it reproduced here about 1 in 20
  runs with Qwen3-0.6B. The model gave all 6 cards the same front ("Photosynthesis"), so only one card survived
  de-duplication. The root cause: `serde_json` (without `preserve_order`) sorted every JSON schema's keys
  alphabetically, so the engine's grammar made models write `back` before `front`, a quiz's `answer` before its
  `question`, and the tutor's `question` before its `step`. That affects every `complete_json` call.
- **What:**
  - `Cargo.toml`: `serde_json` with `preserve_order`, so schemas and all JSON keep the order they're written in.
  - `study.rs`:
    - `reply_json` keeps the finished cards or questions of a reply that ran out of tokens.
    - Cards and quizzes get one retry with more room.
    - Quiz questions are de-duplicated.
    - The prompt asks for a different front on every card.
- **Verify:**
  - `scripts/check-all.sh`;
  - `cargo test cut_off_replies_keep_their_finished_cards`;
  - real engine: `e2e_study` 16/16 with 0.6B (0 retries needed), and the whole e2e suite with 0.6B passes (OCR
    is macOS-only).
- **Undo:** revert. Reverting only the `Cargo.toml` line brings back the alphabetical schemas.

### (this commit) — v0.7.1: the cards in Cloud mode (made on this Mac, written by the cloud)
- **Why:** owner: "Does all these cool features like the recipe thing and the compare thing work on cloud mode too".
  They didn't: Cloud mode sent the message straight to the cluster, whose API is chat only (no structured replies to
  build cards from).
- **What:** `agent.rs`: the card flows move into `specialist()` (used by `run` as before) and `prepare()` runs just
  them, returning `Prepared { sources, notes, kind }`; `cloud_message()` is the question plus the notes.
  `backend.rs`: `Setup` builds the local `Turn` for both paths (no duplicated settings reads); `prepare_for_cloud()`;
  `answer()` for Cloud (not Both, not private) prepares first when a model is loaded here, answers study cards itself
  (`study::reply_for`), sends the cloud the notes, then re-sends the card's sources so [n] match.
- **Verify:** `scripts/check-all.sh` (257 Rust incl. `the_cloud_gets_the_question_and_the_notes`); real engine
  `cargo test e2e_prepare_for_cloud -- --ignored` (a recipe-ideas card and notes, nothing written; a joke prepares
  nothing). The cloud half is the existing, tested `cloud::cmd::send`.
- **Undo:** revert; Cloud mode then sends questions unchanged again.

### (this commit) — tutor mode never solves the problem for you (fix for the red Mac engine test on 2d45eca)
- **Why:** the Mac engine CI job failed `study::tests::e2e_study`: with Qwen3-0.6B, tutor mode's step said "… x = 4"
  and its question echoed "Learner: How do I solve…". The leak guard only knew the calculator's result, and an
  equation has none.
- **What:** `study.rs`: `solve_linear` (one-variable linear equations: 2x + 6 = 14 → x = 4), `states_value` (a hint that
  says "x = <number>", right or wrong, isn't a hint), `parse_tutor` also rejects echoed transcript lines and "questions"
  without a question mark; `tutor_reply` tries twice, then uses a safe built-in opener (`tutor_opener`) instead of a
  free answer. `mac-engine.yml` watches `study.rs`.
- **Verify:** `cargo test study` (the runner's exact reply is a test case); `e2e_study` with Qwen3-0.6B (safe opener)
  and Qwen3.5-2B (a hint and a question); `scripts/check-all.sh`.
- **Undo:** revert.

### (this commit) — v0.7.0: study tools (flashcards with spaced repetition, quizzes, tutor mode)
- **Why:** Phase 7 item 3 (owner: "Lets do it" for Phase 7).
- **What:** `src-tauri/src/study.rs` (new): SM-2 `review` (Again/Hard/Good/Easy = 1/3/4/5), `study_ask` routing
  ("flashcards about…", "quiz me on…", counts), material from attached files / the answer above / the web (3 pages,
  ranked) / the model; `parse_cards` (plain text, deduplicated), `parse_quiz` (answer as text, letter or index, matched
  forgivingly); `ChatEvent::{Flashcards, Quiz}` with BYTE's own short reply (`reply_for`, so the model can't list cards
  or give answers away); decks in DB v7 (`decks`, `cards`, `card_reviews`) with `deck_save`, `decks_list`, `study_queue`,
  `card_review`, `deck_delete`, `card_delete`, `anki_text` (tab-separated Anki import). Tutor mode (`Task::Tutor`):
  `TUTOR_RULES`, a per-message nudge, and `tutor_reply` (JSON feedback/step/question; a hint that contains the final
  answer is rejected and BYTE falls back). `Modules.study` / `settings.studyEnabled`. Commands `decks_list`, `deck_save`,
  `deck_cards`, `study_queue`, `card_review`, `deck_delete`, `card_delete`, `deck_export`.
  UI: `components/study/StudyCards.tsx` (flip cards, Save deck / Study now; quiz that scores itself, Try again, save
  missed as cards), `StudyPanel.tsx` (decks, sessions with Space and 1–4 keys and next-interval previews, card list,
  Anki export), `lib/study.ts` + tests, 🎓 button, Tutor pill, Settings toggle, CSS; screenshots 23–23d.
- **Verify:** `scripts/check-all.sh` (256 Rust incl. SM-2, routing, parsing, deck round trip, Anki export; 121 vitest).
  Real engine (Qwen3.5-2B): `cargo test e2e_study -- --ignored`: 6 flashcards (no LaTeX), a 4-question quiz, and a tutor
  turn that asks back without giving x = 4 away (the 2B model's arithmetic in hints is weak; bigger models are fine).
- **Undo:** revert (DB v7 tables stay, unused). Or Settings → Study tools off.

### (this commit) — v0.6.8: recipes in US cups and spoons, with a US / Metric switch
- **Why:** the owner saw recipes in mL and wants teaspoons, tablespoons, cups and other American measures, or a switch.
- **What:** `src-tauri/src/units.rs` (new): `ingredient_to_us` (mL → tsp/tbsp/cup by size; g → cups via baking
  densities, butter → tbsp, else oz/lb; the metric original kept in the note), `ingredient_to_metric` (uses the
  metric amount already in the note when there is one), `temps` (°C ↔ °F in text, ovens to 25 °F / 10 °C),
  `convert_recipe`, `prompt_rule`. `kitchen::chef(metric)` puts the units rule in the chef prompt; recipes are
  converted after parsing. `settings.measureUnits` ("us" default) → `Turn.metric`. UI: `lib/units.ts` (+ tests), a US /
  Metric toggle on `RecipeCard` (converts from the original each time, so no drift), Settings → Features → Recipe
  measures. Screenshot `19f-recipe-metric`.
- **Verify:** `scripts/check-all.sh`; `cargo test units`; `npx vitest run src/lib/units.test.ts`. Real engine
  (Qwen3.5-2B): `cargo test e2e_kitchen -- --ignored`: ideas, a flat white recipe (oz, fl oz, tbsp) and a meal plan.
- **Undo:** revert. Metric users: Settings → Recipe measures → Metric.

### (this commit) — v0.6.7: reviews, price compare, game hints, self-check, best of 3 (Phase 6 finished)
- **Why:** the owner chose to finish Phase 6 before Phase 7: HANDOFF §8 Phase 6 items 2 (self-check, best-of-3),
  4 (review summarizer, price compare) and 8 (game guides with spoiler-free hints).
- **What:**
  - `reviews.rs` (new): `wants_reviews` (not "review my essay", not "X vs Y", not "worth it to learn…"),
    `subject` (cue/filler stripper, also used by prices), star ratings from AggregateRating/Review JSON-LD,
    shoppers' review texts, `parse_reviews` (known sources only, deduplicated, most-backed first) → `ChatEvent::Reviews`.
  - `prices.rs` (new): `wants_prices` (not maths, tips, weights or puzzles), offers from Product/Offer/AggregateOffer
    JSON-LD or `product:price` meta tags, `matches_product`, `tidy` (one currency, cheapest per store) →
    `ChatEvent::Prices`. The model never produces prices; with none found it's told plainly not to state any.
  - `games.rs` (new): `wants_game_help`, hints JSON → `ChatEvent::Hints`; the model is shown only the first hint.
  - `selfcheck.rs` (new): cited sentences checked against their `[n]` passages after Deep/Extended/fact-check
    answers → `ChatEvent::SelfCheck`.
  - `drafts.rs` (new): best of 3 for reasoning questions in Deep/Extended (`is_reasoning` needs two quantities),
    `final_answer` extraction, majority `pick`, a reconcile pass when all differ; research and the forced search
    are skipped for these questions.
  - Shared: `fetch::{json_ld, ld_is, meta_content}`, `research::read_html_pages`. `agent::Modules` in `Turn` from
    five new settings (default on).
  - UI: `components/chat/ShopCards.tsx` (ReviewsCard, PricesCard, HintsCard, SelfCheckNote), `lib/cards.ts` + tests,
    store events, Activity labels and summary, Settings → Features toggles, CSS; screenshots 22, 22b, 22c.
  - Version 0.6.7, `docs/releases/v0.6.7.md`, VERSIONS, CHANGELOG, HANDOFF (Phase 6 ✅, video-to-slides → Phase 11),
    PROJECT_GUIDE.
- **Verify:** `scripts/check-all.sh` (246 Rust incl. fixtures `product_offer.html`, routing cases; 114 vitest).
  Real engine (Qwen3.5-2B) + web: `BYTE_TEST_WEB=1 cargo test e2e_reviews_prices_hints_drafts -- --ignored`: a reviews
  card (ratings, pros/cons with sources); prices: searches here only reached Wikipedia, so no offers and the answer
  says it couldn't read prices (an earlier run invented one; fixed); a hints card with only the first nudge in the
  answer; the bat-and-ball puzzle in Deep: "3 drafts · 2 agree", $0.05 (was routed to prices before a fix).
- **Undo:** revert. Or switch each off in Settings → Features.

## 2026-09-28

### (this commit) — v0.6.6: the web agent (a private browser BYTE drives, with approval before submitting)
- **Why:** Phase 6 item 7 (owner's plan, round 11): browse and click, fill forms (always stop for approval
  before submitting), download into a folder, full-page screenshots/archives.
- **What:**
  - `src-tauri/src/web_agent/bridge.js` (new, injected into every page): numbered snapshot of links, buttons,
    inputs, lists (hidden ones skipped; private fields and committing buttons marked), `click` (refuses committing
    buttons unless approved; clicks after replying so a page load isn't cut off), `type` (native value setter +
    input/change events; refuses private fields), `choose` (lists and radio groups), `scroll`, `formInfo`, `size`.
  - `web_agent/browser.rs` (new): `TauriBrowser`, a hidden incognito window; commands via `eval`, replies via a
    cancelled navigation to `byteagent://r/<id>?d=<json>` (`parse_reply`); `on_navigation` allows only public
    http(s) (plus about/data/blob), `on_new_window` opens pop-ups in place, page-load counters for waiting.
    `web_agent/capture_mac.rs` (new, macOS): WKWebView `createPDF`, `takeSnapshot` → PNG (window made page-tall
    first), `createWebArchiveData` (type-checked with a scratch crate for aarch64-apple-darwin; compiled on the
    Mac runner). Cargo: `objc2-web-kit`, `block2`, more `objc2-app-kit` features (macOS only).
  - `web_agent/mod.rs` (new): `Browser` trait, `Session` (tools, step limit 25, approval cards with a 10-minute
    wait, a Deny ends browsing for the answer, private-field and public-host guards, downloads ≤200 MB to
    Downloads/BYTE with safe unique names, page saves with a text fallback), `wants_web_agent`, `url_in`,
    `format_snapshot` (+ a next-step nudge), `compact` (older page views shrink to one line), `AGENT_RULES`.
  - `agent.rs`: `Turn.agent`, `Task::Browse`; when it applies the agent gets web_search + calculate + the browser
    tools, opens a linked site first, runs browser tools through the session and closes it at the end
    (`ChatEvent::Browsing`). `chat.rs`: `Approval`, `ApprovalDone`, `Saved`, `Browsing` events (wire test locks
    them; `SavedFile.format`, not `kind`, which is the event tag). Commands `agent_approve`, `agent_show`,
    `agent_file` (only inside Downloads/BYTE; only documents/pictures open, anything else is revealed).
    `settings.webAgentEnabled` (default on).
  - UI: `components/chat/AgentCards.tsx` (approval card, saved-file chips, browsing bar with Show browser),
    `lib/agent.ts` + tests, Agent pill in the composer, Settings → Features toggle, Activity labels, store events,
    CSS; screenshots 21 and 21b. `mac-engine.yml` watches `web_agent/**`.
  - Version 0.6.6, `docs/releases/v0.6.6.md`, VERSIONS, CHANGELOG, HANDOFF, PROJECT_GUIDE.
- **Verify:** `scripts/check-all.sh` (231 Rust incl. 14 session tests with a scripted browser: approvals,
  deny/timeout/cancel, private fields, local hosts, step limit, compaction, routing; 109 vitest incl. the bridge in
  jsdom). Real browser (WebKitGTK under Xvfb): `xvfb-run cargo test e2e_real_browser -- --ignored` (bridge round
  trip, typing, password refused, submit gated, a link click loads example.com). Real engine + real browser:
  `xvfb-run cargo test e2e_web_agent -- --ignored` with Qwen3.5-2B: opened example.com and summarized it; on
  httpbin.org/forms/post typed the name, pressed Submit order → approval card → Deny → nothing sent, browsing
  stopped (before the fix it asked 4 times). Qwen3-0.6B opens and reads pages but doesn't act on forms.
- **Undo:** revert. To switch off without reverting: Settings → Web agent, or `web_agent_enabled` default false.

### (this commit) — v0.6.5: YouTube summaries and questions about a video
- **Why:** Phase 6 item 6 (owner's plan): YouTube transcripts, summaries with timestamps, Q&A.
- **What:**
  - `src-tauri/src/youtube.rs` (new): `video_id` (watch/youtu.be/shorts/live/embed, m./music. hosts), the keyless
    innertube player API (ANDROID, then IOS, then WEB client; the watch page is behind a consent wall here),
    `parse_player` (playability reasons surfaced), `pick_track` (language, human before auto), `parse_timedtext`
    (format 3 with nested `<s>`, legacy `<text>`, double-escaped entities, "[Music]" dropped), `chunks`,
    transcripts cached 24 h (`tools/cache.rs` `TRANSCRIPTS`). Summary: one JSON pass when it fits ~45% of the
    context, else per part and merged → `ChatEvent::Video` (`VideoCard`: TL;DR, key points, chapters) +
    `SUMMARY_RULES` (timestamp links `[mm:ss](https://youtu.be/ID?t=s)`). Q&A: `research::rank_texts` over 90 s
    blocks + `QA_RULES`. `video_ask` also catches follow-ups about a video linked in the last three questions
    (whole-word cues, so "the weather" isn't "he"). Routed first in `agent::run` when the web is on.
  - UI: `components/chat/VideoCard.tsx` (thumbnail → video, timestamps open that moment, Copy summary, PDF),
    `lib/video.ts` + tests, store `video` event, Activity labels; screenshot 20.
  - Version 0.6.5, `docs/releases/v0.6.5.md`, VERSIONS, CHANGELOG, HANDOFF, PROJECT_GUIDE.
- **Verify:** `scripts/check-all.sh` (217 Rust incl. link/player/caption/summary tests on real fixtures; 100
  vitest). `BYTE_TEST_WEB=1 cargo test live_transcript -- --ignored` (60 cues, title, channel). Real engine:
  `cargo test e2e_youtube -- --ignored` (a Video card with 4 chapters and a TL;DR; then a follow-up question).
- **Undo:** revert. To switch off: `youtube::applies` returns false.

### (this commit) — v0.6.4: the Kitchen (recipes, meal plans, recipe box) and Web Off/Auto/Always
- **Why:** owner: "Web should have a auto mode not just on or off"; and weekly meal planning from ingredients on
  hand, with pictures, recipes, coffee and baking, saving picks, "a professional chef… using my hands", searching
  the web when it's on. The original plan's opt-in "Recipes & meal planning" module (K), brought forward.
- **What:**
  - Web mode: `settings.web_mode` ("auto" | "always", serde default auto) beside `web_search` (on/off, so old
    settings still work); `router::wants_web_in(q, always)`, `Turn.web_always`, `research::applies(…, always, …)`;
    UI `lib/web.ts` (Off → Auto → Always pill), `Composer.tsx`, store `toggleWeb`.
  - `src-tauri/src/kitchen.rs` (new): `kitchen_ask` (recipe / ideas / plan), `CHEF` rules, `recipe_ld` (schema.org
    Recipe JSON-LD: @graph, lists, HowToSection, image shapes, ISO durations), web path via
    `fetch::fetch_html` (new; `fetch_page` refactored onto `fetch_body`), `parse_recipe/ideas/plan` (cleaned,
    "you have" marks, pantry left off grocery lists), `ChatEvent::{Recipe, RecipeIdeas, MealPlan}`, recipe box
    (`recipes_list/save/delete`, DB schema v6 `recipes`), "my saved …" lookup. `settings.kitchen_enabled`.
    Routed in `agent::run` after fact-check, before trips.
  - UI: `components/kitchen/{RecipeCard,KitchenCards,RecipeBox}.tsx` (servings scaler, ingredient checklist,
    step timers, Save / Copy / PDF; idea cards → full recipe; week card + grocery list; 📖 recipe box),
    `lib/recipe.ts` (fractions, scaling, text, DocSpec) + tests, Activity labels, Settings → About → Features.
  - Version 0.6.4, `docs/releases/v0.6.4.md`, VERSIONS (YouTube → v0.6.5, web agent → v0.6.6), CHANGELOG, HANDOFF,
    PROJECT_GUIDE; screenshots 19-*.
- **Verify:** `scripts/check-all.sh` (212 Rust incl. kitchen parsing, JSON-LD fixture, recipe box DB; 98 vitest).
  `BYTE_TEST_WEB=1 cargo test live_recipe_page -- --ignored` (BBC Good Food: photo, 9 ingredients, 9 steps).
  Real engine `cargo test e2e_kitchen -- --ignored`: ideas (26 s), recipe (47 s), 3-day plan (23 s) cards all
  produced; the 0.6B test model's recipe content is poor (it called a flat white a cocktail); real use needs the
  8B+ models BYTE recommends.
- **Undo:** revert (DB v6 only adds a table). Turn the Kitchen off in Settings to hide it.

### (this commit) — v0.6.3: faster research, fewer self-imposed limits
- **Why:** owner: "how can we unthrottle stuff for free to make it quicker… remove limits", with nothing that needs
  their time or the cluster.
- **What:**
  - `tools/search.rs`: pacing per engine (DuckDuckGo and Bing each on their own; the cloud's SearXNG,
    Wikipedia and the paper/map services were never paced); search results cached for an hour
    (`tools/cache.rs`, new: bounded TTL + LRU memory cache). Papers (`academic::search`) cached 24 h, places
    (`places::find_at`) 1 h with "open now" recomputed. Pages already had a 24 h cache (`fetch.rs`); page reads
    already stop after 15 s, so no change there.
  - `research::run`: the user's own search runs while the planning request is still being answered.
  - `settings.research_depth` (Settings → Engine → Research: Normal / More / Max) → `research::depth_at`
    (Deep 12/20/32 pages, Extended 24/32/48, batch 6/8/10, extra gap round at Max), `scale_pages` for
    fact-check, compare and trips. `Turn.depth`.
  - Version 0.6.3, `docs/releases/v0.6.3.md`, VERSIONS (YouTube → v0.6.4, web agent → v0.6.5 at the time),
    CHANGELOG, HANDOFF, PROJECT_GUIDE; screenshot 07h.
- **Verify:** `scripts/check-all.sh` (205 Rust incl. cache, pacing and depth tests; 92 vitest). Real engine:
  `e2e_deep_research` 256 s vs 492 s before for the same question (search luck differs between runs, so this is
  indicative; most of the remaining time is the small model reading on CPU).
- **Undo:** revert. Depth defaults to Normal (the old behaviour).

### (this commit) — v0.6.2: places nearby and a trip planner
- **Why:** Phase 6 item 5 (owner's plan): local lookup and trip planning, keyless and private.
- **What:**
  - `src-tauri/src/tools/places.rs` (new): category map → Overpass filters (name search otherwise), Overpass
    mirrors (overpass-api.de, maps.mail.ru, kumi), 1.5 km then 5 km, `parse_overpass` (distance-sorted, dedupe),
    `open_at` (OSM opening_hours: day ranges, overrides, past midnight, 24/7; None for complex rules),
    `spots_text`. `find_places` tool (`tools/mod.rs`, `ToolOutput.places`, `PlacesFound`), forced lookup in
    `agent::run` via `router::places_request`; "near me" uses `settings.home_place` (Settings → About → Your
    town) or asks. `weather::geocode` / `forecast_at` split out.
  - `src-tauri/src/trip.rs` (new): trip read via JSON (`days_in` from the question wins), weather (forecast
    within a week, else last year's dates from the Open-Meteo archive), research + sights, `TripPlan` via JSON
    (`parse_plan` cleans/clamps), `ChatEvent::Trip`, `TRIP_RULES`. `router::wants_trip`. Order in `agent::run`:
    fact-check → trip → compare → research.
  - `commands::calendar_open` (writes an .ics and opens it; only .ics).
  - UI: `Places.tsx`, `Trip.tsx` (day tabs, budget, packing checklist, Save as PDF, Add to Calendar),
    `lib/trip.ts` (DocSpec for the PDF renderer), `lib/ics.ts` (RFC 5545 escape/fold, times), tests; Settings →
    About "Your town"; Activity labels; screenshots 18-*.
  - Version 0.6.2, `docs/releases/v0.6.2.md`, VERSIONS, CHANGELOG, HANDOFF, PROJECT_GUIDE.
- **Verify:** `scripts/check-all.sh` (200 Rust, 92 vitest). `BYTE_TEST_WEB=1 cargo test live_places -- --ignored`
  (real cafés near Pittsburgh). Real engine: `cargo test e2e_trip -- --ignored` (2 days in Pittsburgh → a Trip
  event with 2 days and a cited summary). The Open-Meteo archive refuses this container (shared-IP daily limit);
  it works from a home connection.
- **Undo:** revert. To switch off: `trip::applies` returns false; `router::places_request` returns None.

### (this commit) — v0.6.1: fact-check and compare & decide
- **Why:** Phase 6 item 4 (owner's plan): trustworthy answers to "is it true…" and help choosing between options.
- **What:**
  - `src-tauri/src/factcheck.rs` (new): claims (one for a short question, up to 5 via JSON for longer text) →
    per claim a plain search and a rebuttal search (+ papers for research claims) → pages per claim → passages
    ranked per claim (≤2 per source) → `FACT_RULES` (verdict table with exact quotes and [n], confidence line).
  - `src-tauri/src/decide.rs` (new): `options_from_question` ("X vs Y", "should I get X or Y", "compare X, Y and
    Z"), criteria + weights via JSON (prompt echoes rejected), a search per option + head-to-head, scores via
    JSON (`parse_scores`: fuzzy names, clamped 1–10, only real source numbers), `ChatEvent::Decision`, then
    `DECIDE_RULES` for the written recommendation.
  - `research.rs`: shared helpers made `pub(crate)` (`Ctx`, `run_searches`, `read_pages`, `find_papers` with a
    tag, `rank_texts`, `notes_budget`, `lenient_json`, `pick_n`). `router::{wants_fact_check, wants_compare}`.
    `agent::Task` (`FactCheck`) on `Turn` and `ChatRequest.task`; `agent::run` order: fact-check → compare →
    research → forced search. Confidence rule wording fixed (the small model copied "reason" literally).
  - UI: `Decision.tsx` (score table, weight sliders, live totals, Copy), `lib/decide.ts` + tests,
    `markdown.ts` `markVerdicts` + badge CSS, Activity labels, shield **Fact-check** button
    (`MessageView.factCheckPrompt`, task on the user message so Regenerate keeps it), store `decision` event.
  - Version 0.6.1, `docs/releases/v0.6.1.md`, VERSIONS, CHANGELOG, HANDOFF, PROJECT_GUIDE; screenshots 17-*.
- **Verify:** `scripts/check-all.sh` (191 Rust, 88 vitest). Real engine + web with Qwen3-0.6B:
  `cargo test e2e_fact_check -- --ignored` (claims → searches → verdict table) and `cargo test e2e_compare --
  --ignored` (options "MacBook Air M4" / "Dell XPS 13", a Decision event, a recommendation). The 0.6B model's
  verdicts are unreliable (it called the 10% brain myth True); real use needs the 8B+ models BYTE recommends.
- **Undo:** revert. To switch one off: `factcheck::applies` / `decide::applies` return false.

### (this commit) — Phase 6 part 1 (v0.6.0): deep research, research papers, confidence line, citation styles
- **Why:** Phase 6 (Research+), owner priority "answer quality and research depth win". Deep/Extended mode did
  one search and read 4–5 pages; now it researches properly and cites papers.
- **What:**
  - `src-tauri/src/research.rs` (new): plan searches (JSON via `chat::complete_json`) → parallel searches →
    `academic_search` when `router::wants_papers` or the plan says so → read 12/24 pages, 6 at a time,
    alternating between searches (`interleave`) → split into passages, rank by words (`lexical_score`) and by
    meaning (nomic embeddings via `AppState.embedder`, fused with RRF) → `pick` (≤3 per source, ~half the
    context) → Extended: `gaps` + more searches → notes + `REPORT_RULES` (TL;DR, sections, [n], a
    `**Confidence:**` line). Called from `agent::run` in place of the forced first search when
    `research::applies`.
  - `src-tauri/src/tools/academic.rs` (new): Crossref, Europe PMC, arXiv (keyless; OpenAlex and Semantic
    Scholar refuse keyless requests from shared IPs), merged and de-duplicated; fixtures in `tests/fixtures/`.
  - `tools/mod.rs`: `Source.meta` (`SourceMeta`: authors, year, venue, doi), `SourceBook::add_paper`,
    `academic_search` tool (Deep/Extended only, `specs(…, papers)`), `papers_text`.
  - `chat::complete_json` (moved from `docs::ask_json`), `router::wants_papers`, `agent::Turn.app`,
    `fetch::without_ref_marks` (drops pages' "[12]" footnotes, which the model cited as sources in testing).
  - UI: `src/lib/citations.ts` (APA, MLA, Chicago, Harvard, IEEE, BibTeX) + tests; `Activity.tsx` research
    step labels, paper cards ("Paper · year · venue"), **Cite** menu; `markdown.ts` `markConfidence` +
    `.confidence` badge CSS; screenshot scenes `16-deep-research`, `16b-cite-menu`.
  - Version 0.6.0 (`bump.mjs`), `docs/releases/v0.6.0.md`, `docs/VERSIONS.md`, CHANGELOG, HANDOFF, PROJECT_GUIDE.
- **Verify:** `scripts/check-all.sh` (182 Rust tests, 85 vitest). Real engine + web:
  `BYTE_TEST_WEB=1 BYTE_TEST_LLAMA_SERVER=… BYTE_TEST_MODEL=Qwen3-0.6B-Q8_0.gguf cargo test e2e_deep_research -- --ignored --nocapture`
  (passed: 4 searches, 6 papers, 8 pages, 32 passages from 12 sources, cited report with a confidence line);
  `BYTE_TEST_WEB=1 cargo test live_papers -- --ignored` (8 papers from Europe PMC + Crossref).
- **Undo:** revert. To keep the code but switch the pipeline off, make `research::applies` return false
  (Deep/Extended then use the single forced search again).

### (this commit) — Versions follow the phases (v0.5.0 = Phase 5), with a description per release
- **Why:** owner: name builds after phases instead of "test.N" ("Phase 1 improvements v0.1…"), and describe
  each version on the repo so people can choose one with fewer features.
- **What:** `docs/VERSIONS.md` (scheme + every version, old test builds mapped to v0.1.0–v0.4.0),
  `docs/releases/v0.5.0.md` (release title + description), `release.yml` reads `docs/releases/<tag>.md` for
  the release title/body; `node scripts/bump.mjs 0.5.0`; CHANGELOG, CLAUDE.md, HANDOFF, README updated.
- **Undo:** revert; tags already published stay.

### (this commit) — Windows icon (icon.ico) so the Windows compile check gets past tauri-build
- **Why:** the Windows CI job stopped at "icons/icon.ico not found; required for generating a Windows Resource".
- **What:** `src-tauri/icons/icon.ico` (16–256 px, made from `icons/icon.png`), listed in `tauri.conf.json`.
- **Undo:** remove the file and the list entry.

### a85f1ea — Phase 5 (light): documents made on this Mac, saved as PDF, PowerPoint or Word
- **Why:** owner decision: the cloud stays the main document maker; the Mac gets a lighter version for offline,
  private chats and people without a cloud invite.
- **What:** `src-tauri/src/docs.rs` (new) + commands `doc_outline`/`doc_write`/`doc_save`; `src/lib/docs/`
  (new: `spec.ts`, `pdf.ts`, `pptx.ts`, `docx.ts`, `charts.ts`, `render.ts`, tests + `samples.test.ts`
  generator); `components/documents/LocalDocs.tsx` (new), `DocumentsPanel.tsx` (This Mac / BYTE Cloud),
  `Shell.tsx` (button always), `api.ts`, `app.css`; npm: `pdfmake`, `pptxgenjs`, `docx`, `chart.js`,
  `@types/pdfmake`; screenshots `15-*`.
- **Verify:** `scripts/check-all.sh`; `BYTE_TEST_LLAMA_SERVER=… BYTE_TEST_MODEL=<Qwen3-0.6B> cargo test
  e2e_plans_and_writes -- --ignored`; `GEN_DOCS_DIR=/tmp/docs npx vitest run src/lib/docs/samples` then open the
  files. On a Mac: 📄 → This Mac → Plan it → Write it → Save as PDF / PowerPoint / Word.
- **Undo:** `git revert a85f1ea`.

### (this commit) — CI fixes: Windows check uses Strawberry Perl; releases keep the .dmg as an artifact
- **Why:** the Windows job failed configuring SQLCipher's bundled OpenSSL (Git Bash perl lacks modules);
  the test.14 release built but couldn't publish ("Resource not accessible by integration": the repo's
  workflow token is read-only, Settings → Actions → General → Workflow permissions).
- **What:** `ci.yml` windows job sets `PERL` to Strawberry Perl; `release.yml` uploads the .dmg as a run
  artifact (`if: always()`).
- **Undo:** revert the two workflow edits.

### (this commit) — CI: the Rust core must compile on Windows
- **Why:** owner decision: Mac phases first, but keep the core portable for the Windows/Linux apps later.
- **What:** `.github/workflows/ci.yml` job `windows` (`windows-latest`, `cargo check --all-targets` with a
  placeholder sidecar). No app changes.
- **Undo:** delete the job.

### (this commit) — Version 1.0.0-test.14 (Phase 4: files, scans, photos, knowledge base, instant answers)
- **What:** `node scripts/bump.mjs 1.0.0-test.14`; CHANGELOG "Unreleased" → test.14. CI and the Mac engine test
  (OCR + embeddings on Metal) green on `4410b53`. Release: `release.yml` with `tag=v1.0.0-test.14`.
- **Undo:** bump back to test.13.

### 33488f7 — Phase 4: instant answers for questions asked before
- **Why:** Phase 4 item 8 (instant-answer cache, moved from Phase 2): speed for repeated questions without
  risking stale answers.
- **What:** `src-tauri/src/answer_cache.rs` (new: `cacheable`, `put`, `find`, `clear`), `db.rs` schema v5,
  `backend.rs` `reuse_earlier_answer`, `commands.rs` `answer_cache_put`/`answer_cache_clear` +
  `ChatRequest.fresh`, `settings.rs` `answer_cache`, `tools::Source` Deserialize, `embed.rs` threshold e2e.
  UI: `store.ts` `rememberAnswer` + `fresh` on Regenerate, `api.ts`, `types.ts`, `KnowledgeTab.tsx` toggle.
- **Verify:** `scripts/check-all.sh`; `cargo test answer_cache`; real model:
  `cargo test e2e_instant_answer_threshold -- --ignored` (with `BYTE_TEST_EMBED_MODEL`).
- **Undo:** `git revert 33488f7`, or turn "Instant answers" off in Settings → Knowledge base.

### 902024a — Phase 4: knowledge base, search by meaning, "My files", reader view
- **Why:** Phase 4 items 3–7 (owner: "keep going with phase 4"): BYTE should answer from the user's own folders
  and cite them.
- **What:**
  - `src-tauri/src/embed.rs` (new): `Embedder` (own `Engine::helper`, `embed.pid`), `embed()` with nomic
    prefixes, normalization, byte packing, cosine; idle stop after 5 min. `engine.rs`: `LaunchOpts.embedding`
    (`--embedding --pooling mean`, FA auto, f16 cache, chat flags removed, no warm-up), `Engine::helper`.
  - `src-tauri/src/kb.rs` (new) + `db.rs` schema v4 (`kb_sources`, `kb_files`, `kb_chunks`, `kb_fts`;
    `Db::conn` now `pub(crate)`): walker, `chunk`, `rrf`, `fts_query`, store/delete files, `index`,
    `embed_pending`, `search`, `schedule` (launch + every 15 min).
  - `commands.rs`: `kb_status`, `kb_add`, `kb_remove`, `kb_reindex`, `kb_search`. `settings.rs`: `kb_enabled`.
    `state.rs`: `embedder`, `app` (OnceLock<AppHandle>). `lib.rs`: modules, reaping/killing `embed.pid`.
  - `tools/mod.rs`: `SEARCH_FILES` tool, `file_url`, `specs(web, memory, files)`, `ToolContext.files`.
    `agent.rs`: `Turn.files`, forced first file search (`router::wants_files`). `backend.rs`: files when
    `kb_enabled` and passages exist. `fetch::worth_reading` only http(s).
  - UI: `settings/KnowledgeTab.tsx`, `reader/Reader.tsx`, `lib/reader.ts` (+ test), store (`kb`,
    `kbProgress`, `refreshKb`, `toggleFiles`, `reader`), Composer "My files" pill + wrapping bar, Activity
    labels + file sources open the reader, sent-file chips open the reader.
  - `mac-engine.yml`: embedding model download + `BYTE_TEST_EMBED_MODEL`; paths. Screenshots `14-*`.
- **Verify:** `scripts/check-all.sh`; `cargo test kb::` and `BYTE_TEST_LLAMA_SERVER=… BYTE_TEST_EMBED_MODEL=…
  cargo test e2e_embeddings -- --ignored`; screenshots 14, 14a–c. On a Mac: Settings → Knowledge base → Add
  folder, download "search by meaning", ask "what does my … say about …".
- **Undo:** `git revert 902024a` (schema v4 tables stay in existing databases, unused; harmless). To switch the
  feature off without reverting: `kbEnabled: false` in settings.

### 9b50c29 — Phase 4: read scanned PDFs and text in photos (Apple Vision)
- **Why:** Phase 4 item 2: scans had "no readable text"; photos of documents only helped models that can see.
- **What:** `src-tauri/src/ocr.rs` (new; `objc2-vision`, `objc2-pdf-kit`, `objc2-app-kit`, `objc2-foundation`
  under the macOS target in `Cargo.toml`): `image_text`, `pdf_text` (PDFKit renders each page → TIFF → Vision,
  max 60 pages). `files.rs`: `has_text_layer`, `scanned_pdf_text`, photos' words read, `Ingested.ocr`,
  `for_model` includes photo text; test helper `tests::test_pdf`. `chat.rs`: note for models that can't see.
  `mac-engine.yml` paths. UI: `LocalFile.ocr`, chip label.
- **Verify:** `scripts/check-all.sh`; Mac: `mac-engine.yml` runs `ocr::tests::e2e_reads_text_from_a_rendered_page`
  (renders a PDF page and reads "Invoice number 48213" back). Attach a scanned PDF in the app.
- **Undo:** `git revert 9b50c29` (or make `ocr::image_text`/`pdf_text` return Err to switch it off).

### 567584e — Phase 4 part 1: files and photos in local chats, models that see images
- **Why:** the owner said "keep going with phase 4": BYTE should read the user's files, and photo buttons should
  appear only when a model that can see is loaded (owner request, 2026-09-28).
- **What:**
  - `src-tauri/src/files.rs` (new): `kind_of`, `ingest` (PDF by page via `pdf-extract`; docx/pptx/xlsx/odt/odp/ods
    via `zip` + `quick-xml`; HTML via `fetch::extract`; text/code; photos → data URL, HEIC/WebP converted and
    photos over 2 MB shrunk to 2048 px with `/usr/bin/sips` on macOS). Caps: 25 MB file, 240k chars, 8 MB photo.
    `for_model` builds `<file name=…>` blocks from `relevant_passages`. Cargo: `pdf-extract`, `quick-xml 0.37`,
    `zip`; dev `lopdf 0.42`.
  - `chat.rs`: `ChatMessage.files` / `images`, `with_files` (called in `backend.rs` before `fit_history`),
    `question_text` (router/agent see only the user's words), image parts in `base_messages`, `IMAGE_TOKENS`.
  - `engine.rs`: `LaunchOpts.mmproj` → `--mmproj`, memory for the adapter, `Endpoint.vision`,
    `EngineStatus::Ready.vision`; the last fallback launch drops the adapter.
  - `models.rs`: `Vision`, `VISION_QUANT`, `download_dir` (adapters in `models/vision/<id>/`), `vision_path`,
    `ModelStatus.vision`; `download_target` handles `"<id>:vision"`. `commands.rs`: `file_ingest`; download and
    delete use `download_dir`. `tune.rs` `launch_opts` sets `mmproj` when downloaded.
  - `scripts/build-catalog.mjs`: `pickVision` (F16, else BF16, else Q8_0; Q8_0 beats a 16-bit file over
    1.5 GB); `--vision` re-checks the catalog in place. `models.json`: 128 models with `vision`.
  - UI: `store.ts` (`pendingFiles`, `attachLocal`, `Message.files`, `toWire` sends files, image-reader download
    restarts the engine when it's the main model's), `Composer.tsx` (paperclip + drop for local chats; photo
    types only when `engine.vision`), `Attachments.tsx` (`LocalFileChips`, `fileDetail`), `MessageView.tsx`,
    `ModelCard.tsx` ("Sees images" tag, `VisionRow`), `types.ts` (`LocalFile`, `FileKind`), `api.ts`.
  - `tools/ui-shots`: `file_ingest` mock, vision state, download mock uses `key`; shots `13-local-files*`.
- **Verify:** `scripts/check-all.sh`; `cargo test files chat::tests::attached image_adapter fallback`;
  screenshots `13-local-files.png`, `13b-local-files-sent.png`, `13c-sees-images.png`. On a Mac: download a
  model's image reader, attach a photo, ask about it; attach a PDF and ask about it.
- **Undo:** `git revert 567584e`. To turn only photos off: have `tune::launch_opts` leave `mmproj` as `None`.

### c0f9649 — Repo public: automatic CI back on; version 1.0.0-test.13
- **Why:** the owner made the repo public (Actions free for public repos).
- **What:** `ci.yml` on every push (skips docs-only), `mac-engine.yml` on engine-related pushes again; CLAUDE.md
  and HANDOFF updated; `node scripts/bump.mjs 1.0.0-test.13`; CHANGELOG "Unreleased" → test.13.
- **Undo:** set the workflows back to `workflow_dispatch` only.

### 93c2a4a — Build on your own Mac: scripts/build-mac.sh
- **Why:** GitHub Actions minutes are at 100%, so release builds can't run on GitHub.
- **What:** `scripts/build-mac.sh`: checks/installs tools (Xcode CLT, Homebrew cmake + node, rustup), builds
  llama-server with Metal (`build-llama-server.sh`), `npm ci`, `tauri build --target aarch64-apple-darwin`,
  verifies the engine is in the bundle and the ad-hoc signature, prints the .dmg path; `--install` copies to
  /Applications and clears quarantine. CLAUDE.md and HANDOFF mention it.
- **Verify:** on an Apple Silicon Mac: `scripts/build-mac.sh --install`. (Syntax-checked here; this container is Linux.)
- **Undo:** delete the script.

### 72ce820 — Catalog: 713 models, community fine-tunes, a details dropdown
- **Why:** the owner asked for ~250 more regular models, the community models left out before (with their
  creator and what they're good for), and a dropdown per model with details and use ideas.
- **What:**
  - `scripts/discover-models.mjs`: looser official filters (older generations, language/domain models, more
    quantizer accounts) → 480 official (was 320); `--community` pass → `scripts/catalog-community.json`: 90
    community models (uncensored capped at 45, one per base model and size; Dolphin; story/role-play tunes;
    repos marked `not-for-all-audiences` and explicit names excluded) and 130 independent models from smaller
    makers (listed as regular).
  - `scripts/enrich-catalog.mjs` (new): `details` per chat model: `about` from the original model's card (never
    a quantizer's card; boilerplate, bios, setup text filtered), `author`, `sourceUrl`, `strengths` 1–5,
    `ideas`, `community`, `caution`. Card starts cached in `scripts/catalog-cards.json` (git-ignored).
  - `scripts/build-catalog.mjs` merges the community list. `src-tauri/catalog/models.json`: 713 models, 1.4 MB.
  - Rust `models.rs`: `ModelDetails`, `CatalogModel::is_community`; `recommend` never picks community models.
    Test `embedded_catalog_is_valid_and_small` now: 600+ chat models, < 2 MB, details for all, authors for 80%+.
  - UI: `ModelCard.tsx` Details dropdown (`ModelDetailsView`), Community badge, "by <creator>";
    `CatalogBrowser.tsx` Community / Stories / Uncensored chips (community models hidden from the main list
    unless those or a search are used); search covers descriptions and creators.
  - `tools/ui-shots`: details come from the current catalog; new shot `07g-model-details.png`.
- **Verify:** `scripts/check-all.sh`; `cargo test community_models_are_never_recommended`; screenshots.
- **Undo:** revert the commit; or rebuild the old catalog with the previous `catalog-discovered.json`.

### 90ded06 — Documents faster, "quit these apps" help, CI manual, work log
- **Why:** the owner found the cloud document maker slow; asked that a model that won't load say which open
  apps use memory and which can be closed; Actions minutes nearly used up; wanted everything documented.
- **What:**
  - `src/components/chat/Attachments.tsx` `cloudImageCached`: one image cache for library photos and document
    pages; a failed load is retried next time. `documents/DocumentsPanel.tsx`: page 1 isn't downloaded twice.
    (The shared cloud HTTP client from 9f854b3 also removes a TLS handshake per page image.)
  - `src-tauri/src/memory.rs` (new): apps by memory (helpers grouped under their `.app`; macOS and BYTE left
    out), `advice()` for error messages, `quit_app()` (normal quit via AppleScript, only apps in the report).
    Commands `memory_report`, `app_quit`. `engine.rs` adds the advice to "didn't start" / "keeps stopping".
    `src/components/MemoryHelper.tsx`: the list with Quit buttons and "Try loading again" under the engine
    error in the chat box.
  - `.github/workflows/ci.yml`: manual only. `scripts/check-all.sh`: every CI check, locally.
  - `docs/WORKLOG.md` (this file).
- **Verify:** `scripts/check-all.sh`; `cargo test memory`; screenshot `tools/ui-shots` → `12-memory-helper.png`.
- **Undo:** revert the commit; the memory helper is self-contained (memory.rs + MemoryHelper.tsx + 2 commands).

### 52dc8fb — Handoff: speed and reliability list
- **What:** `docs/HANDOFF.md` §3.2d, the ordered speed list. Docs only.

### 9f854b3 — Big MoE models load, cloud streams smoothly, CI uses fewer minutes
- **Why:** Qwen3.6 35B-A3B errored on the owner's 16 GB Mac though BYTE said it would run; cloud chat felt slow
  and jerky; Actions minutes.
- **What:**
  - `system.rs` `plan_offload`: 4 GB left for macOS (was 3) and 1 GB GPU headroom (was 0.3) when part of a model
    runs on the CPU. Versions that can't fit are no longer offered (35B-A3B IQ3_XXS on 16 GB).
  - `engine.rs` `Engine::start`: fallback ladder (no helper → half context + ¼ more on the CPU → 4k context with
    every expert / half the layers on the CPU), remembered per session (`Inner.safer`); offloaded models cap
    `ubatch` at 512. `fallback_launches` has unit tests.
  - `cloud/mod.rs`: `http_client()` shared via `AppState.cloud_http` (connection reused); `follow()` reopens a
    stream the server closed mid-answer at once; only empty attempts back off and count toward giving up.
  - `MessageView.tsx` + `lib/throttle.ts`: markdown re-rendered ~12×/s while streaming; `Sidebar.tsx`
    re-renders only when `listSignature` changes.
  - Workflows: CI once per push, mac-engine manual (both later made manual).
- **Verify:** checked locally that Qwen3.6 35B-A3B UD-IQ2_M loads (8 s) and answers with BYTE's flags; all 58
  catalog architectures exist in llama.cpp b11205. Tests: `models::tests::big_moe_models_run_partly_on_the_cpu`,
  `engine::tests::a_model_that_fails_to_load_gets_safer_settings`,
  `cloud::tests::a_stream_closed_after_every_piece_keeps_going_without_waiting`.
- **Undo:** the memory plan is two constants in `system.rs` (`OFFLOAD_OS_RESERVE`, `OFFLOAD_GPU_MARGIN`); the
  ladder is the loop at the end of `Engine::start`.

### 1af0d51 — Version 1.0.0-test.12 (release built: web search fixes)

### 409cd84 — Web search goes through the BYTE cloud's SearXNG first
- **What:** `CloudClient::search` (`GET /api/search`), `tools::search::cloud_search` (relevance filter, rests 5 min
  after a 429, 10 after a 401, 1 after a failure), used first when a key is saved and the chat isn't private.
  Contract: `docs/CLOUD-MODE.md` "Web search".
- **Undo:** pass `cloud: None` in `backend.rs` `local_turn` to go back to keyless search only.

### 7f814f4 — Web search: drop junk results, add Wikipedia and a real weather forecast
- **What:** `search::on_topic` junk filter, Wikipedia search alongside (`search::wikipedia`), DuckDuckGo 2-minute
  rest after a bot check, `tools/weather.rs` (Open-Meteo), `router::weather_place`, reads the latest search's
  results first (`agent::read_top` `from`).

### 2ccb36c — Web search: search first for real questions, read pages, no tool-call leaks
- **What:** `router::wants_web`, `agent::read_top` (BYTE reads pages itself), search budget + repeat guard,
  `ANSWER_NOW` step, `agent::ToolTextFilter`, `fetch::worth_reading`, query cleanup + follow-up topics.
- **Verify:** `agent::tests::e2e_web_quality_report` (real engine + internet): 1/8 → 7/8 usable answers.

### fc0f6f3 — Live web-search quality report test (ignored)

### 9b84a4f — Version 1.0.0-test.11 (release built: workspaces, themes, logo, ModelBackend)

### 14f03ba — ModelBackend: one boundary for answering a turn
- **What:** `backend.rs` (`ModelBackend`, `LocalLlama`, `Cloud`, `with_fallback`); `chat_send` calls
  `backend::answer`.

### c45edf8 — Themes: byte-ai's five-color system, Midnight default, 20 themes
- **What:** `styles/tokens.css` (five tokens per theme, derived with `color-mix`), `design/themes.ts`,
  `styles/tokens.test.ts` (contrast for every theme).

### 1e27bf7 — Logo: BYTE's eight-bit mark, not a bolt
- **What:** `design/Logo.tsx`, app icon + favicon regenerated from `src-tauri/icons/app-icon.svg`.

### 098ecad — Cloud and Both workspaces replace the cloud toggle
- **What:** `settings.workspace`, sidebar switch, cloud chat list/open/new/delete, Both answers side by side
  with "Keep this one", budgets chip, 429 text, re-read after a dropped stream, invite-only onboarding path.

Older history: `CHANGELOG.md` (per test build) and `docs/HANDOFF.md` §8 (every phase).
