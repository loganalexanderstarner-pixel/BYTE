# Troubleshooting

<!-- mac -->
**"BYTE can't be opened because Apple cannot check it"**
Right-click BYTE in Applications → **Open** → **Open Anyway**. Or System Settings → Privacy & Security → **Open Anyway**. BYTE isn't signed by a paid Apple account, so macOS asks once.
<!-- win -->
**"Windows protected your PC" when opening BYTE**
Choose **More info**, then **Run anyway**. BYTE isn't signed with a paid certificate, so Windows SmartScreen asks once.
<!-- all -->

<!-- mac -->
**Mac control or "Hey BYTE" stopped working after an update**
macOS asks again for permissions after each update. System Settings → Privacy & Security → Automation / Microphone / Accessibility: turn BYTE off and on.
<!-- win -->
**"Hey BYTE" or dictation stopped working**
Open Settings → Privacy & security → Microphone and make sure **Let desktop apps access your microphone** is on, then restart BYTE.
<!-- linux -->
**"Hey BYTE" or dictation stopped working**
Open your desktop's Sound settings → Input, check the right microphone is selected and not muted, then restart BYTE. If BYTE runs as a Flatpak or Snap, allow it to use the microphone in its permissions.
<!-- all -->

<!-- win -->
**BYTE doesn't answer when I say "Hey BYTE"**
1. Open [Settings → Voice and Hey BYTE](byte-setting:voice) and check **Listen for "Hey BYTE"** is on and a speech model is installed.
2. Check Windows lets BYTE use the microphone (Settings → Privacy & security → Microphone).
3. If another program is using the microphone for a call, or a full-screen game is running, BYTE stays quiet on purpose; see [Interrupting BYTE, calls and games](help:interrupting-calls-games).

**No sound from BYTE**
Check the volume and that the right speakers are chosen in Settings → System → Sound. BYTE follows a changed default device. If it stays silent after you plug in or switch a headset, choose the output again in Windows, then ask BYTE once more or restart BYTE.

**A game stutters while BYTE is on**
In [Settings → Models and speed](byte-setting:models) set where the model lives to the processor only, and in Voice and Hey BYTE set **Answers from** to Automatic with BYTE Cloud connected.

**Updates**
Open [Settings → General](byte-setting:general) and press the update button when a new version is offered. If an update fails, download the newest installer and run it over the old one.
<!-- all -->

**Answers are slow**
Close apps using lots of memory, or pick a smaller model in [Settings → Models and speed](byte-setting:models). Speed boost should be on.

**The model won't load**
Check free disk space and memory in [Settings → Models and speed](byte-setting:models). Try **Restart engine**.

**A download stopped**
Downloads resume where they left off: press **Resume**.

<!-- mac -->
**An update fails with "Read-only file system".** macOS is running a read-only copy of BYTE because it was downloaded and isn't signed by Apple. Quit BYTE, make sure it's in Applications (not the .dmg window), run `xattr -cr /Applications/BYTE.app` once in Terminal, open BYTE again and install the update.

**BYTE says it needs the microphone, but isn't in System Settings → Microphone.** Versions before 0.12.5 couldn't ask macOS for the microphone. Update BYTE (Settings → General), then press the mic: macOS asks once. If you answered "Don't Allow" earlier, switch BYTE on in System Settings → Privacy & Security → Microphone, or reset the answer in Terminal with `tccutil reset Microphone com.loganstarner.byte` and try again. Because BYTE isn't signed by Apple, macOS may ask again after an update.
<!-- win -->
**BYTE says it needs the microphone.** Open Settings → Privacy & security → Microphone, turn on **Microphone access** and **Let desktop apps access your microphone**, then press the mic in BYTE again.
<!-- linux -->
**BYTE says it needs the microphone.** Open your desktop's Sound settings → Input, check the right microphone is selected and not muted, then try again.
<!-- all -->
