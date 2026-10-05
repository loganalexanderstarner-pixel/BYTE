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

## CUDA is the large part, and it buys about 12%

| | |
|---|---|
| `cublasLt64_13.dll` | **470 MB** |
| `cublas64_13.dll` | 52 MB |

`cudart` is statically linked, so that is the complete list.

Measured on the test GPU (a 16 GB NVIDIA desktop card) with Qwen3 4B Q4_K_M: the same
model, the same llama.cpp tag, built twice from the same source, a **2,781-token prompt
with the prompt cache off**, two rounds each (within 1% of each other), 2026-10-05:

| | CUDA | Vulkan | Vulkan is |
|---|---|---|---|
| generation | 213 tok/s | 194 tok/s | 9% slower |
| **prompt reading** | **12,600 tok/s** | **11,090 tok/s** | **12% slower** |

**Correction.** This section used to say Vulkan read prompts 39 times slower (9,827
against 251 tok/s) and that this was why the installer bundles CUDA. That figure was
wrong; the first measurement of it is unexplained and was never repeated. It was found by
repeating the test with the same model through both engines. BYTE's speed estimates for
every AMD and Intel card were built on it (`chip.rs`, now corrected), and so was the
installer's size. Lesson kept: **a number that drives a design decision gets measured
twice, on a long input, with caches off, before it is written down.**

What that leaves is a trade-off, not a necessity, and it is **the owner's call**:

| | Ship CUDA + Vulkan (today) | Ship Vulkan + a small CPU build |
|---|---|---|
| Installer | about 564 MB | about 60-80 MB (estimate) |
| NVIDIA speed | fastest | about 10-12% slower |
| Engine build in CI | about 2 hours (CUDA, five architectures) | minutes |
| Other NVIDIA generations | CUDA is the mature path | **unmeasured**: only one card was tested, and older cards without cooperative-matrix support may lose more on Vulkan |
| No GPU or no driver | CPU through the CUDA build | CPU build (needs an AVX2 baseline build to be written) |

**Decided (owner, 2026-10-05): keep CUDA bundled.** About 1 GB installed is acceptable for what the
app does, and "everything works" matters more than the installer's size. Both engines ship, as
before. The cost to keep in mind: an auto-update re-downloads the whole installer, so updates need a
smarter path (not built yet). One correction came out of the discussion and applies either way:
the CUDA build starts at the Turing generation (RTX 20, GTX 16), so older NVIDIA cards (GTX 900 and
10 series, the Titan X, older Quadro and Tesla, the MX150 to MX350) are sent to the Vulkan engine
(`gpu::cuda_can_run`); before, they would have been given a CUDA build with no kernels for them.

Until that was decided, both engines shipped as before, and nothing here changes what the
installer carries. The owner's decision of 2026-10-04 to bundle rather than fetch on demand
stands either way.

## Both engines ship

The CUDA build and the Vulkan build, side by side. Vulkan covers AMD and Intel
-- including hardware we have no way to test on -- and needs no SDK at runtime,
because the user's own graphics driver provides the implementation.

One configuration to handle: **a GPU can be present and unusable.** The test machine's
AMD integrated graphics was missing from `vulkaninfo` and from DXGI, and this section used
to blame a missing Vulkan driver. It was simply **disabled in Device Manager** (problem code
22) with the AMD driver installed. Enabled, the AMD Windows driver provides Vulkan, the
Vulkan engine lists it beside the NVIDIA card, and the model ran on it correctly (Qwen3 0.6B:
generating about 19-27 tok/s, reading a long prompt at 232 tok/s). A card that is present
but cannot be used still deserves a clear message (update or enable the driver) rather than a
silent CPU fallback, but this machine was not an example of one.

With an integrated AMD chip *and* the NVIDIA card both active, BYTE picks the NVIDIA card and
CUDA (checked), and the Vulkan engine on its own also ignores the weak integrated chip: the
same speed with and without `--device` pinned to the discrete card (11,074 against 11,061
tok/s reading a prompt).

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
