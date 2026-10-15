# Safe mode and what happens after a crash

## Safe mode
Safe mode starts BYTE with every optional feature that listens, looks at the screen or controls things **switched off for that run**: Hey BYTE, the browser, the clipboard history, the selection shortcut, the Messages inbox, the terminal, automations and connectors,
<!-- mac -->
and Mac control.
<!-- win -->
and PC control, guided help, screen questions and the replay, routines and finding files.
<!-- linux -->
and the other control features.
<!-- all -->

Your saved settings are **not changed**. When you restart normally, everything is back the way you chose it. While safe mode is on, a banner at the top says so, and turning one of those features on is refused until you restart normally.

### How to start it
- In [Settings → Backup and help](byte-setting:backup), press **Restart in safe mode**.
- Or start BYTE with the option `--safe`.
<!-- win -->
  On Windows, make a copy of the BYTE shortcut, open its Properties and add `--safe` after the path in **Target**.
<!-- mac -->
  On a Mac, run `open -a BYTE --args --safe` in Terminal.
<!-- linux -->
  Add `--safe` to the command that starts BYTE.
<!-- all -->

To leave safe mode press **Restart normally** on the banner (or in Settings), or just close BYTE and open it again from its normal shortcut.

### When to use it
If BYTE keeps closing, or something you switched on behaves badly, safe mode shows whether an optional feature is the cause. If BYTE works in safe mode, turn the features back on one at a time.

## After BYTE closes unexpectedly
BYTE keeps a small marker file while it runs and removes it when it quits normally. If the marker is still there at the next start, BYTE did not quit normally, and it asks:

**"BYTE closed unexpectedly last time."** with these choices:
- **Prepare a problem report:** the same report as in Settings → Backup and help, plus anything BYTE wrote about the error when it stopped. You read every line first, can edit or delete any of it, and save it to a file. Nothing is sent anywhere.
- **Restart in safe mode.**
- **Not now.**

Switching the computer off, restarting it, or an update that restarts BYTE is not counted as a crash.
