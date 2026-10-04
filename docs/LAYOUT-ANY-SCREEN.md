# Any screen, including foldables

Owner instruction, 2026-10-04: *"I want it to adapt to screen size even
foldables for android so no matter the screen it looks amazing on any device."*

This file is for whoever builds UI next on any platform. It is written because
the current approach cannot meet that instruction, and because the cheapest
moment to change it is before Android UI work starts rather than after.

## What is there now

Measured in `src/styles/app.css`:

| | |
|---|---|
| `max-width` media queries | 8 |
| distinct breakpoints | **560, 600, 640, 760px** |
| narrowest rule | **560px** |
| `pointer: coarse` / `hover: none` handling | **none** |

And `src-tauri/tauri.conf.json` sets `minWidth: 900`.

## Why that cannot work

**Nothing below 560px.** A modern phone is 390–430 CSS px. A **folded Galaxy Z
Fold cover screen is roughly 320–380px** — the narrowest screen the owner
actually has, and there is no rule for it.

**Four breakpoints is device-class thinking** — phone, tablet, desktop. A
foldable breaks that model by being one device at three widths, and Android
split-screen and free-form windows produce arbitrary widths that belong to no
class. You cannot enumerate your way to "any device".

**Touch is unhandled.** Any affordance that appears on `:hover` is unreachable
on a touchscreen, and nothing currently distinguishes the two.

## What to do instead

**Container queries, not viewport breakpoints.** Components respond to the space
*they* have. The same chat panel then works at 320px folded, 700px unfolded and
1240px on a desktop with one set of rules, and keeps working in split-screen
where the viewport tells you nothing useful about the panel.

**Fluid type and spacing** with `clamp()`, so sizes scale continuously instead
of jumping at 560px. One scale in `tokens.css`, every component reading from it.

**`pointer: coarse` and `hover: none`** as first-class states: 48dp minimum
touch targets, and no control that only reveals itself on hover.

**`dvh` rather than `vh`** for anything full-height, so the Android keyboard and
browser chrome cannot crop the composer. `docs/ANDROID.md` already requires that
the keyboard must not cover the composer; `dvh` is how.

## The Android-specific piece that is easy to miss

**Folding is a configuration change, and Android recreates the activity by
default.** Without handling it, unfolding mid-conversation loses UI state —
scroll position, a half-typed message, an open panel. Jetpack WindowManager
reports posture (folded, half-open, tabletop, unfolded) and the layout should
react to it. This belongs in the Kotlin plugin `docs/ANDROID.md` already plans.

`docs/ANDROID.md` lines 129-132 already describe the intent — one column folded,
sidebar as a drawer, tablet layout unfolded, 48dp targets. What is missing is
that the CSS implements device classes, so that intent currently has nothing to
build on.

## It helps the desktop too

This is not Android-only work. `minWidth: 900` exists because components assume
a wide viewport; once they respond to their own width, a desktop window can be
dragged far narrower and a Windows user can keep BYTE in a side-by-side split.
Same system, three platforms.

## Who should do it

The React UI is shared, and `app.css` is 4,700 lines being actively edited by
the session that owns the Mac app. This should be **one deliberate pass by
whoever owns the UI**, not incremental edits from two sessions at once — which
is why this is a written approach rather than a half-applied patch.
