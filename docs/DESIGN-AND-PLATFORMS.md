# Design system and cross-platform plan

Two things Logan asked for, written down so both sessions work from the same
source.

## What is changing, and what is NOT

Logan's words: *"I like the different themes the macOS app has. I just like
the setup the web app has — the colours, the branding — vs the weird B. But
the button setups, the layout, is great."*

So this is **not** "make it look like the web app".

| | decision |
|---|---|
| **Layout, button arrangement, command-deck home** | **Keep.** He likes it. Do not restyle toward the web app. |
| **Having many themes** | **Keep.** The 20-theme plan stands. |
| **Colour palette** | **Adopt byte-ai's**, as the default/base theme and as the model for how tokens are structured. |
| **Logo** | **Replace the letter-B mark** (`src/design/Logo.tsx`, currently a drawn `B`) with the eight-bit mark below (done). |

In short: **byte-ai's skin on the app's bones.** The structure you have is
right; the palette and the mark are what should come across.

---

## Part 1 — the design system, from byte-ai

These are the real values from `byte-ai/app/static/styles.css`, not an
approximation from screenshots. Use them for the default theme and as the
pattern for the rest — **not** as a reason to reduce the theme count.

### Palette

Everything is driven by five tokens. Set those per theme and the rest follows.

| token | midnight (default) | paper (light) |
|---|---|---|
| `--bg` | `#0f1420` | `#f4f6fb` |
| `--panel` | `#171d2c` | `#ffffff` |
| `--border` | `#2a3247` | `#dde3ee` |
| `--text` | `#e7ebf3` | `#1a2133` |
| `--accent` | `#4c8dff` | `#3b6fd9` |

Other shipped themes — same five tokens, different values:

| theme | bg | panel | border | text | accent |
|---|---|---|---|---|---|
| terminal | `#000000` | `#0a0f0a` | `#1f3d1f` | `#33ff33` | `#33ff33` |
| steelers | `#0a0a0a` | `#161616` | `#333333` | `#f5f5f0` | `#ffb612` |
| ocean | `#071a1f` | `#0d2830` | `#164048` | `#e0f5f8` | `#2dd4bf` |
| paper | `#faf6ee` | `#ffffff` | `#e0d5c0` | `#2b2418` | `#b8763f` |

**In BYTE (done 2026-09-28):** `src/styles/tokens.css` follows this: `:root` holds Midnight's five values and
every derived token; each theme block sets only the five (+ optional `--accent-2`, and the few exact overrides
High Contrast needs). 20 themes; `src/styles/tokens.test.ts` checks text ≥ 4.5:1 on bg/panel/surface-2,
secondary text ≥ 4.5, faint text and the accent ≥ 3.0, and button labels on the accent ≥ 4.5. To pass that,
Paper's accent is `#96592a` (byte-ai's `#b8763f` gave white labels only 3.4:1). Light themes use white
labels on the accent.

### Derived values — don't hardcode these

Everything else is computed from the five, which is why a new theme is five
lines rather than fifty:

    --surface-1:   color-mix(in srgb, var(--panel) 92%, var(--bg))
    --surface-3:   color-mix(in srgb, var(--panel) 86%, var(--text) 4%)
    --hairline:    color-mix(in srgb, var(--border) 70%, transparent)
    --accent-soft: color-mix(in srgb, var(--accent) 14%, transparent)
    --accent-ring: color-mix(in srgb, var(--accent) 40%, transparent)

    --r-sm: 8px   --r-md: 12px   --r-lg: 18px   --r-xl: 26px

    --shadow-2: 0 4px 16px -4px var(--shadow-soft)
    --shadow-3: 0 18px 48px -20px var(--shadow)

### The logo

**Eight bits: one byte.** Two rows of four rounded squares; two are lit (top
row second, bottom row third), the other six are the same colour at about 30%
opacity. It sits to the left of the `BYTE` wordmark. (An earlier version of
this doc described a lightning bolt; that was wrong, corrected by the owner
2026-09-28 with a screenshot of the real mark.)

It is drawn in the **live theme's accent**, so it recolours with the theme
instead of being a fixed asset that clashes in half of them. In the app:
`src/design/Logo.tsx` (`LIT_BITS = [1, 6]`). The app icon
(`src-tauri/icons/app-icon.svg`, rendered with `npx tauri icon`) uses the same
grid in gold (`#f5bd3c`) on a near-black rounded square, since an icon can't
follow the theme.

```svg
<svg viewBox="0 0 50.5 23.5" aria-label="BYTE">
  <!-- cell 10, gap 3.5, rx 2.6; lit cells at full opacity, the rest 0.32 -->
  <rect x="0"    y="0"    width="10" height="10" rx="2.6" fill="var(--accent)" fill-opacity="0.32"/>
  <rect x="13.5" y="0"    width="10" height="10" rx="2.6" fill="var(--accent)"/>
  <rect x="27"   y="0"    width="10" height="10" rx="2.6" fill="var(--accent)" fill-opacity="0.32"/>
  <rect x="40.5" y="0"    width="10" height="10" rx="2.6" fill="var(--accent)" fill-opacity="0.32"/>
  <rect x="0"    y="13.5" width="10" height="10" rx="2.6" fill="var(--accent)" fill-opacity="0.32"/>
  <rect x="13.5" y="13.5" width="10" height="10" rx="2.6" fill="var(--accent)" fill-opacity="0.32"/>
  <rect x="27"   y="13.5" width="10" height="10" rx="2.6" fill="var(--accent)"/>
  <rect x="40.5" y="13.5" width="10" height="10" rx="2.6" fill="var(--accent)" fill-opacity="0.32"/>
</svg>
```

### Two rules worth keeping

**Every colour is defined in the base `:root`** before any theme overrides it.
A colour whose only definition lives inside a theme block is invisible in the
others — that is the classic unreadable-UI bug.

**Contrast is resolved, not assumed.** byte-ai picks ink by measuring it
against the surface behind it. A brand colour chosen for a header band is not
automatically readable as body text: coral on butter yellow measured 2.59:1,
below the 3.0 large text needs. If the app themes text over coloured
surfaces, do the same.

---

## Part 2 — Windows and Linux

### This supersedes a locked decision

`CLAUDE.md` currently says, under non-negotiables:

> macOS only, Apple Silicon only. No Windows/Intel work.

**Logan has changed this.** The target is macOS, Windows and Linux — three
desktop platforms, each feeling native to its own OS rather than identical
across all three.

**Android is a target** (owner, 2026-10-04): a phone app for devices with up to 16 GB of RAM, running models on
the phone. See `docs/ANDROID.md`.

**iOS/iPadOS is not a target.** Considered and dropped on 2026-09-27;
`PROJECT_GUIDE.md`'s original "no iPhone app" line stands. Don't design for
it, don't leave hooks for it.

### Owner decisions, 2026-09-28: one native app per OS, and any hardware

**Order and testing (2026-09-28):** finish the Mac phases first, not all three in parallel. Meanwhile CI
compiles the Rust core for Windows too, so Mac-only code can't creep into shared modules (anything
Mac-specific stays behind `#[cfg(target_os = "macos")]` with a clear fallback, like `ocr.rs`). The owner will
give access to their gaming PC for real GPU testing when the Windows phase starts (CI runners have no GPUs).
Rough size: Windows ≈ 40–50% of the Mac effort (GPU/VRAM planner, CUDA/Vulkan engine builds, Phase 9–10
integrations rebuilt natively), Linux ≈ 30–40% after that.

**Separate apps, each built around its own OS.** Windows and Linux get the same BYTE (layout, look, name,
chat, cloud, modules, everything the Mac app has), but each is its own app that does things the way that OS
allows, and does *more* where the OS gives more access. Nothing should be "macOS-shaped" on Windows or Linux.
Examples to plan from when those phases start (the owner will add ideas then):

| | macOS (today's plan) | Windows | Linux |
|---|---|---|---|
| App control | AppleScript / JXA, Shortcuts | UI Automation, COM (Office, Outlook), PowerShell | D-Bus, xdotool/ydotool, desktop scripting |
| System | EventKit, Keychain, Vision OCR | Windows Credential Manager, Windows.Media.Ocr, WinRT APIs | Secret Service (libsecret), Tesseract, freedesktop portals |
| Widgets / quick access | menu-bar popover, floating widget | taskbar + Windows widgets board, jump lists, toast actions | tray, GNOME Shell / KDE Plasma widgets |
| Automation | launchd, Shortcuts | Task Scheduler, Power Automate hand-off | systemd timers, cron |
| Engine | llama.cpp Metal | llama.cpp CUDA / Vulkan / CPU | llama.cpp CUDA / ROCm / Vulkan / CPU |

**Any hardware, the user's choice.** PCs vary: system RAM, one or more GPUs with their own VRAM (NVIDIA, AMD,
Intel), different CPUs. BYTE must use all of it well and let the user choose: **"Best automatically"**
(default: fill VRAM, put the rest in RAM), **"GPU only"**, **"System RAM / CPU only"**, or **"Use both"** with a
split they can adjust. The planner grows from `system::plan_offload` (already splits MoE experts and dense
layers between GPU and CPU) into a per-device plan (llama.cpp `--tensor-split`, `--main-gpu`, `-ngl`,
`--n-cpu-moe`), with the same honesty rules: never say a model runs when it won't.

### Do the abstraction now, the ports later

Recommended, and worth arguing with if you disagree: **define the boundary
now, port after v1.0.** Phases 5–12 otherwise bake in macOS assumptions that
cost more to undo than to prevent — but building three apps while still
designing one would triple the work at the worst possible time.

### The engine layer is the whole problem

`Engine::spawn` starts `llama-server` as a child process via
`tauri_plugin_shell`. That works on all three desktop platforms, but the
binary, its path, and its acceleration flags differ on every one — and cloud
mode is not a spawned process at all.

That last point is what makes the abstraction necessary even with iOS off the
table: a remote HTTP engine and a local child process are two implementations
of the same thing, and chat code should not be able to tell them apart.

So the boundary that matters is roughly:

    trait ModelBackend {
        async fn complete(&self, req: Request) -> Stream<Token>;
        fn capabilities(&self) -> Caps;     // context, vision, tools
    }

with three implementations:

| backend | platform | notes |
|---|---|---|
| spawned `llama-server` | macOS (Metal) | what exists today |
| spawned `llama-server` | Windows (CUDA / Vulkan), Linux (CUDA / ROCm / CPU) | different binary, different path, different acceleration |
| **cloud (BYTE)** | all | see `docs/CLOUD-MODE.md` — already built and live |

If chat code talks to that trait, a new platform is a new backend. If it
talks to `Engine` directly, each platform is a rewrite.

### What is genuinely different per OS

**Not** the UI — that is one React app and should stay one.

| area | macOS | Windows | Linux |
|---|---|---|---|
| acceleration | Metal | CUDA, Vulkan fallback | CUDA / ROCm / CPU |
| signing | ad-hoc today | Authenticode (or SmartScreen warning) | none needed |
| packaging | `.app` / `.dmg` | MSI or NSIS | AppImage, `.deb` |
| secret storage | Keychain | Credential Manager | Secret Service / libsecret |
| OS integration | Shortcuts, Automation | Registry, Task Scheduler | freedesktop, systemd user units |
| autostart | LaunchAgent | Run key / Task Scheduler | `.desktop` in autostart |

Tauri abstracts window management and updates; it does **not** abstract the
engine binary, acceleration, or where a secret is kept.

### Build dependencies for Linux

Confirmed working on this machine (Ubuntu 24.04):

    libdbus-1-dev libsoup-3.0-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev

Without `libdbus-1-dev` the Rust build fails in `libdbus-sys` before reaching
any project code — the error names a build script, not the app, so it looks
worse than it is.

### Keep the honesty about limits

`PROJECT_GUIDE.md` already states plainly that a 14B local model does not
reason at Claude's level. Windows and Linux machines vary far more than
Apple Silicon does — a 4090 will outrun an M4 badly, a laptop iGPU will be
far slower. The RAM planner in `system.rs` needs a VRAM equivalent, and the
same honesty: say what this machine will actually do before downloading nine
gigabytes onto it.

---

## Distribution without paying Apple (or Microsoft)

Logan's constraint, stated 2026-09-27: **no paid developer subscriptions.**
No Apple Developer Program, no Windows code-signing certificate. This is a
hard requirement, not a preference, and it changes the answer on one platform
completely.

### Desktop: fine, with a first-run speed bump

| platform | unsigned reality | friction |
|---|---|---|
| **Linux** | No signing gate exists. AppImage: `chmod +x` and run. `.deb` installs normally. | **none** |
| **macOS** | Ad-hoc sign locally (`codesign -s -`) for free. No notarization, so Gatekeeper warns on first open. | one-time, per machine |
| **Windows** | Unsigned `.exe`/`.msi` runs. SmartScreen shows "Windows protected your PC". | one-time, per download |

The macOS bypass is worth getting right in the release notes, because **the
old advice is wrong now**: right-click → Open stopped working as a Gatekeeper
bypass in macOS Sequoia. The current path is *System Settings → Privacy &
Security → Open Anyway*, after attempting to open once. From a terminal,
`xattr -d com.apple.quarantine /Applications/Byte.app` also clears it.

Windows: *More info → Run anyway*. SmartScreen reputation accrues with
download volume, which at two users it never will — so the warning is
permanent, and the README should just say so rather than implying it fades.

Don't buy a Windows cert either. OV certificates run a few hundred dollars a
year and **do not remove the warning** on their own — only EV certificates get
immediate reputation, and those cost more and need a hardware token.

### There is no iOS section

Dropped 2026-09-27, and the cost side is why: free Apple provisioning issues a
**7-day** certificate with a 3-app device limit, so a sideloaded build stops
launching every week. That is not an app you hand to someone, and the only
capability a native iOS app would add over reaching byteai.bytebylogan.xyz in
Safari is running a model on the device — which a phone cannot usefully do.

So nothing of value is lost, and the roadmap keeps three platforms instead of
four. If Logan wants phone access later, the web app is already there.
