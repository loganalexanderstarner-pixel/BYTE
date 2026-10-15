## What to test in this build

1. Close the phone and open BYTE again: the model loads by itself, or **Load it** on the banner loads it.
2. In Cloud mode, send a message right after waking the phone: it answers on the cloud, or the banner says why and
   **Try the cloud again** works.
3. Settings → Models: no Fara in the list, and the details and Model Lab say "phone", not "Mac".
4. Settings → About / Privacy / Appearance: nothing says "this Mac" or "your Mac", and the Mac-only items are gone
   (Mac control and upkeep toggles, Keyboard and menu bar, Touch ID lock, iMessage inbox, iCloud backup, Mac permissions).
5. Attach a file (a PDF, a text file, a photo) to a chat and ask about it; also export something (Save as…): both should work
   (new: files come back as links on Android and are now copied in). Voice, the Agent button, folders for the knowledge
   base and picking a model file are hidden on the phone on purpose.
6. Tiny model (ERNIE 0.3B): say "Hey there" (no more "Welcome to my friendly AI assistant…"), then ask a web question: the answer
   shows "This model is very small…" with **Answer with BYTE Cloud** and **Get a bigger model**; no Fact-check button.
   Also try a 4B+ model on the same question: that is what accurate answers need.
7. Voice input (new): Settings → Voice shows the speech model download; tap 🎤, allow the microphone when Android asks, say
   a sentence: it is typed into the message box. Reading aloud, "Hey BYTE" and speaker labels are still Mac-only.
8. Texts (new): Settings → Privacy → Messages inbox: switch it on, tap Allow (three Android prompts: read texts, send texts,
   contacts). Tap the speech-bubble button: your conversations with names; open one; Draft a reply; Reply: the other phone gets it.
   Have someone text you while BYTE is open: a notification appears (within about 15 seconds). Texts only arrive while BYTE is open.
9. Notifications (new): Settings → Privacy → Notifications: tap Allow (Android 13+ asks once). Then close BYTE completely and have
   someone text you: a notification with their name appears (the words are hidden on the lock screen); tap it to open BYTE.
   Reminders and daily briefings use the same permission.
10. APK size: about 60 MB (the last one was 142 MB).
11. Share: in any app (a web page, a note), Share → BYTE: BYTE opens on a new chat with the text in the box (not sent).
12. Widget: long-press the home screen → Widgets → BYTE → "Ask BYTE" bar; tapping it opens BYTE on a new chat, box ready.
13. Calendar: Settings → Privacy → Notifications and calendar → Allow the calendar; run a daily briefing ("brief me"): today's events are listed.
14. Assistant (new): Settings → Privacy → "BYTE as your phone's assistant" → Open settings → choose BYTE as the digital assistant.
    Then hold the power button (or swipe up from a corner) in any app: BYTE opens on a new chat with "What's on my screen?" and the
    words on screen in the box (not sent). Also: select text in any app → the menu shows BYTE → it lands in the box.
15. Camera (new): with a vision-capable model (or on the Cloud), a camera button sits next to the paperclip: tap it, take a photo,
    it attaches; ask about it. (Reading text in photos with a model that can't see isn't available on Android.)
16. Fold (new): half-fold the Fold like a laptop (screen split top/bottom) with BYTE open: the chat should sit in the top half and the box
    (and keyboard) in the bottom half, nothing across the hinge. Unfold or fold flat: back to normal. Tell me what looks wrong.
17. If anything fails, tap **Copy diagnostics** (About) and send it with your report.
