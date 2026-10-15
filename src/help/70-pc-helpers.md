# Everyday helpers: region, explain, error windows, copy toast

<!-- mac -->
The region screenshot and the error window helper are part of the Windows app for now. **Explain this** and the **Smart copy toast** work on the Mac too: turn them on in [Settings → Control and screen](byte-setting:control) under **Everyday helpers**.
<!-- linux -->
These helpers aren't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
Small helpers that work in any app. Find them in [Settings → Control and screen](byte-setting:control) under **Everyday helpers**. **Each one is off until you turn it on**, has its own switch, and is only for you on this computer. None of them run in a private chat or in kids mode.

## Where questions are answered
The **Ask where** choice covers all the helpers:
- **Local** (the default) answers on this computer.
- **Auto** answers on this computer and only falls back to BYTE Cloud for text that did not come from your screen.
- **Cloud** answers in BYTE Cloud. It is the only choice that lets text from your screen leave this computer, and only the words, never a picture.

## Region screenshot
Press the shortcut (Ctrl+Alt+R unless you changed it) and drag a box. BYTE reads only that box, in memory, turns it into text and forgets the picture; nothing is saved. A small popup then offers **What's this?**, **Translate**, **Copy the text**, **Make a note** and **Ask a question**.
- If any window under the box is a password manager, a bank, a private browser window or on your never-look-at list, nothing is read.
- It reads text, not pictures: a box around a photo comes back empty.

## Explain this
Select text in any app and press the shortcut (Ctrl+Alt+E). A short explanation appears in a small popup next to the pointer. The popup does not take the keyboard from the app you were in, and no chat window opens. It copies the selection with Ctrl+C and puts your clipboard back, so it will not do that in a terminal or code editor (a copy there could stop a running command), over a full-screen game, or in a window on your never-look-at list.

## Error window helper
When a new error or installer window appears, the tray icon gets a red dot and its tooltip says "BYTE can explain this". Click the icon (or the small notice) and BYTE reads the words the window itself shows, using Windows accessibility, and says what happened and what to try. It never reads password fields. Some windows do not share their words; for those, use the region screenshot.

## Smart copy toast
When you copy a parcel tracking number, a street address, a link or a one-time code, a small toast offers the useful thing: track the parcel, show the address on a map, open the link or copy it without tracking parameters, or clear the code from the clipboard. It works on this computer only and never appears for text that looks like a password or a key.

## Dictate anywhere
Hold the shortcut (Ctrl+Alt+D), speak, and let go: the words are typed into the app you are in. The microphone is open only while you hold the key, and a speech model on this computer turns it into text (install one in Settings → Models → Voice). BYTE never types into a password box, a full-screen game, a window on your never-look-at list or a private chat, and it never types a new line, so it cannot press Enter for you. Pick a letter, number or F key for the shortcut.

## If it does not work
- No answer: load a model in Settings → Models, or choose Auto or Cloud.
- The shortcut does nothing: another program may use it. Pick other keys next to the switch.
- A window "came back black": some apps block screen capture.
<!-- all -->
