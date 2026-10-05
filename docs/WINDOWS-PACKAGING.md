# Packaging BYTE for Windows

What the installer has to carry, and why. Measured on a development PC rather than
assumed.

## The MSVC runtime DLLs are not optional

`llama-server.exe` links these, and **none of them ship with Windows**:
`MSVCP140.dll`, `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll`, and `VCOMP140.DLL`
(OpenMP, used by the CPU backend).

The proof they are not part of Windows: on the test machine Windows was
installed on **2026-05-15** and all four files are dated **2026-05-27** --
twelve days later -- with a VC++ x64 redistributable registered under
`HKLM\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes`. They came from a
redistributable, so a clean machine does not have them.

Without them BYTE installs, launches, and fails to start its engine with a
missing-DLL error naming a file the user has never heard of.

Two ways to fix it: bundle the four files as resources, or have the installer
run `VC_redist.x64.exe`. Bundling is preferable -- it needs no elevation and
cannot fail halfway.

They are Microsoft's redistributable binaries, so they are **copied at build
time and never committed**. `scripts/fetch-windows-runtime.ps1` stages them into
the gitignored `vendor/windows-runtime/` from Visual Studio's redist folder or
System32, and fails the build loudly if any is missing -- an installer without
them installs cleanly and then cannot start its engine, which is far worse than a
failed build.

**They are listed in `tauri.windows-release.conf.json`, not in the base Windows
config, on purpose.** Tauri fails the build when a listed resource is missing
(the sidecar placeholders found that out), so putting them in the base config
would break every dev and test build on a machine that had not run the fetch
step. The release build passes it explicitly:

    powershell scripts/fetch-windows-runtime.ps1
    npm run tauri build -- --config src-tauri/tauri.windows-release.conf.json

**Not yet verified:** that the resource lands next to the executable, which is
where Windows looks for a DLL first. It has to be checked by installing the
built package, not assumed from the config.

## CUDA is the large part, and it earns its place

| | |
|---|---|
| `cublasLt64_13.dll` | **470 MB** |
| `cublas64_13.dll` | 52 MB |

`cudart` is statically linked, so that is the complete list.

Measured on the test GPU with Qwen3 4B Q4_K_M -- the same model, the same
llama.cpp tag, built twice from the same source:

| | CUDA | Vulkan |
|---|---|---|
| generation | 204.8 tok/s | 192.1 tok/s |
| **prompt (1,701 tokens)** | **9,827 tok/s** | **251 tok/s** |

Generation is a rounding error apart, 1.07x. Prompt processing is **39x**. A
10,000-token document is about one second against about forty, and documents
and web research are most of what BYTE does -- so a Vulkan-only build would be
visibly worse for the majority of users, who are on NVIDIA. The owner's
decision of 2026-10-04 is to bundle rather than fetch on demand, so the app
works instantly the way the Mac one does.

## Both engines ship

The CUDA build and the Vulkan build, side by side. Vulkan covers AMD and Intel
-- including hardware we have no way to test on -- and needs no SDK at runtime,
because the user's own graphics driver provides the implementation.

One real configuration to handle: **a GPU can be present and still have no
Vulkan driver.** The test machine's AMD integrated graphics has no Vulkan ICD
registered at all, so `vulkaninfo` enumerates only the NVIDIA card. That needs
a clear message telling the user to update their graphics driver, not a silent
fall back to CPU that leaves them wondering why it is slow.

## Window chrome

`tauri.conf.json` sets `titleBarStyle: Overlay` and `hiddenTitle: true`, both
**macOS-only**. Windows ignores them and draws its standard title bar, with the title
from the config. That is the right behaviour and needs no Windows override.

**Do not override `app.windows` in `tauri.windows.conf.json`.** Tauri merges platform
configs as JSON merge patches, and a merge patch **replaces arrays wholesale instead of
merging them**. An earlier version of that file listed one window with two keys, and on
Windows the window silently lost its title, its 1240x820 size, its minimum size and its
centring: it opened as "Tauri App" at about 800x600. Nothing failed and no test noticed;
it was found by taking a screenshot of the running app. The same rule explains why
`tauri.windows-release.conf.json` lists every `externalBin` and resource in full.

The title bar's colour is a separate matter: Windows draws its own, light by default,
which is a bright strip across a dark app. `lib/titlebar.ts` sets the native theme from the
BYTE theme's real background colour, on Windows only, so the Mac's overlay is untouched.
Matching the Mac exactly (no system title bar, controls drawn in React) is shared-frontend
work and is deliberately left as a later step.

## WebView2

Built into Windows 11. `downloadBootstrapper` covers Windows 10 machines that
lack it and adds about 2 MB, against roughly 130 MB for an embedded copy.
