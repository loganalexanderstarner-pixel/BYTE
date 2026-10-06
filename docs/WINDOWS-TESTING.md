# Trying BYTE on Windows

A test build, not a release. It installs for the current user (no administrator rights), carries
every engine it needs (NVIDIA CUDA, Vulkan for AMD and Intel, a CPU fallback) and works offline once
a model is downloaded.

## Installing

1. Run `BYTE_<version>_x64-setup.exe`. Windows will probably say it **"protected your PC"**, because
   the build is not signed with a paid certificate. Choose **More info**, then **Run anyway**.
2. Open BYTE. The first screens look at your PC, recommend a model that fits it, and download it
   (about 11 GB for the recommended one on a 16 GB graphics card; the progress screen shows speed and
   time left, and **Pause** and resuming after quitting both work).
3. Start chatting. The pill at the top shows the model and its context size when it is ready.

BYTE keeps its data in `%APPDATA%\com.loganstarner.byte`. To start from nothing, quit BYTE and delete
that folder (the models are in its `models` folder and are the large part).

## What is worth trying

- Ask something; watch the speed under the answer. **Deep** and **Extended** modes research on the web.
- **Settings → Models**: other models, with how well each fits this PC.
- Attach a PDF or a photo of text; BYTE reads it (Windows' own text recognition).
- Voice: press the microphone (Windows will ask once for permission), or try spoken answers.
- **Settings → Hotkeys and clipboard → PowerShell commands** (off until you turn it on): ask "run a
  command to show my disk space". BYTE shows the command and what it does and runs it only after you
  press **Do it**.
- **Help** (the lifebuoy at the top): articles worded for Windows.
- Select text in any app and press **Ctrl+Alt+B**: it opens in BYTE to rewrite, fix or reply to.

## Known to be rough

- **Web search** without a BYTE cloud key uses DuckDuckGo, which sometimes shows a bot check to a home
  connection after many searches; answers then fall back to weaker sources. Connect your BYTE cloud key
  (**Settings → Cloud**) for the full search.
- The Mac-only "control your computer" features (Reminders, Mail, Messages, Mac upkeep, Shortcuts) are
  not on Windows; the settings that belong to them are hidden.
- **Windows Hello lock** is available but its prompt has only been checked for availability, not used.
- Updates: the check runs, but installing an update downloads the whole installer (about 560 MB).
- Not testable here: Intel graphics, NPUs, and an AMD graphics card on the Windows driver (an AMD
  integrated chip was tested; see `docs/WINDOWS-PACKAGING.md`).

## Reporting something

Say what you did, what you expected and what happened; a screenshot helps. **Settings → Engine** has
the engine's log, which is the first thing to look at if a model will not load.
