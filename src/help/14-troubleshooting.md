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
<!-- all -->

**Answers are slow**
Close apps using lots of memory, or pick a smaller model in [Settings → Models](byte-setting:models). Speed boost should be on.

**The model won't load**
Check free disk space and memory in [Settings → Engine](byte-setting:engine). Try **Restart engine**.

**A download stopped**
Downloads resume where they left off: press **Resume**.

<!-- mac -->
**An update fails with "Read-only file system".** macOS is running a read-only copy of BYTE because it was downloaded and isn't signed by Apple. Quit BYTE, make sure it's in Applications (not the .dmg window), run `xattr -cr /Applications/BYTE.app` once in Terminal, open BYTE again and install the update.

**BYTE says it needs the microphone, but isn't in System Settings → Microphone.** Versions before 0.12.5 couldn't ask macOS for the microphone. Update BYTE (Settings → About), then press the mic: macOS asks once. If you answered "Don't Allow" earlier, switch BYTE on in System Settings → Privacy & Security → Microphone, or reset the answer in Terminal with `tccutil reset Microphone com.loganstarner.byte` and try again. Because BYTE isn't signed by Apple, macOS may ask again after an update.
<!-- win -->
**BYTE says it needs the microphone.** Open Settings → Privacy & security → Microphone, turn on **Microphone access** and **Let desktop apps access your microphone**, then press the mic in BYTE again.
<!-- all -->
