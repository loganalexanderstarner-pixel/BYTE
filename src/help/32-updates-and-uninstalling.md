# Updates, what's new and removing BYTE

## Updates
BYTE checks once a day for a newer version (you can turn this off in [Settings → General](byte-setting:general)) and shows a banner when there is one. **Install** downloads it, checks that it is signed by BYTE's own key, installs it and restarts BYTE. Your chats, settings and models are kept.

## What's new
The first time you open BYTE after an update, a card shows what changed in that version, taken from the release notes built into the app, so it works offline. Press **Got it** and it does not show again. A fresh install shows no card.

## Removing BYTE
<!-- win -->
The Windows uninstaller has a checkbox, **Delete the application data**.
- **Unticked** (the default) removes the program only. Your chats, notes and models stay in BYTE's data folder, so installing BYTE again finds them.
- **Ticked** also deletes your chats, settings and downloaded models. This cannot be undone.

Either way the uninstaller always removes the **startup entry**, so BYTE no longer opens when you sign in (an update does not remove it).

For finer control, use [Settings → Backup and help](byte-setting:backup) → **Remove BYTE's data** before uninstalling. It lets you remove the chats and settings, the models, or both. It lists exactly what it will delete, asks you to confirm, and then closes BYTE so you can uninstall it.
<!-- mac -->
Drag BYTE from Applications to the Bin. To remove your chats and models too, first use [Settings → Backup and help](byte-setting:backup) → **Remove BYTE's data**.
<!-- linux -->
Remove BYTE with your package manager. To remove your chats and models too, first use [Settings → Backup and help](byte-setting:backup) → **Remove BYTE's data**.
<!-- all -->
