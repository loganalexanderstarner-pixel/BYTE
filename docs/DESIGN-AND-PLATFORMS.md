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
| **Logo** | **Replace the letter-B mark** (`src/design/Logo.tsx`, currently a drawn `B`) with byte-ai's bolt-in-rounded-square below. |

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

A rounded square with a bolt cut through it, filled with an accent gradient.
It is drawn in the **live theme's accent**, so it recolours with the theme
instead of being a fixed asset that clashes in half of them.

```svg
<svg viewBox="0 0 32 32" fill="none" aria-hidden="true">
  <defs>
    <linearGradient id="bm" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="var(--accent)"/>
      <stop offset="1" stop-color="var(--accent)" stop-opacity="0.55"/>
    </linearGradient>
  </defs>
  <rect x="3.2" y="3.2" width="25.6" height="25.6" rx="8" fill="url(#bm)"/>
  <path d="M17.9 7.6 10.4 17.6h4.4l-1.2 6.9 7.6-10.1h-4.4z" fill="#fff" fill-opacity="0.96"/>
  <rect x="3.2" y="3.2" width="25.6" height="25.6" rx="8"
        stroke="#fff" stroke-opacity="0.16" stroke-width="1.1"/>
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

**Logan has changed this.** The target is macOS, Windows and Linux, and
sideloadable iOS/iPadOS later. Each should feel native to its own OS, not
identical across three.

### Do the abstraction now, the ports later

Recommended, and worth arguing with if you disagree: **define the boundary
now, port after v1.0.** Phases 5–12 otherwise bake in macOS assumptions that
cost more to undo than to prevent — but building three apps while still
designing one would triple the work at the worst possible time.

### The engine layer is the whole problem

`Engine::spawn` starts `llama-server` as a child process via
`tauri_plugin_shell`. That is fine on desktop and **impossible on iOS**,
which does not permit launching arbitrary executables.

So the boundary that matters is roughly:

    trait ModelBackend {
        async fn complete(&self, req: Request) -> Stream<Token>;
        fn capabilities(&self) -> Caps;     // context, vision, tools
    }

with four implementations over time:

| backend | platform | notes |
|---|---|---|
| spawned `llama-server` | macOS (Metal) | what exists today |
| spawned `llama-server` | Windows (CUDA / Vulkan), Linux (CUDA / ROCm / CPU) | different binary, different path, different acceleration |
| **cloud (BYTE)** | all | see `docs/CLOUD-MODE.md` — already built and live |
| in-process llama.cpp / MLX | iOS, iPadOS | the only option where spawning is banned |

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
