# Voice helpers: navigation, read aloud, live captions and your own wake phrase

<!-- mac -->
**Read aloud with highlighting** works on the Mac too (turn it on in [Settings → Control and screen](byte-setting:control) under **Everyday helpers**). Voice navigation and live captions are part of the Windows app for now, and the wake phrase below is chosen in the same place.
<!-- linux -->
These helpers aren't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
Find these in [Settings → Control and screen](byte-setting:control) under **Everyday helpers**. Each is **off until you turn it on**, belongs to you on this computer, and stays off in kids mode and private chats.

## Voice navigation (accessibility)
Turn on **Voice navigation** (PC control must be on too). Then say, to BYTE: "go back", "go forward", "scroll to the bottom", "scroll down 5", "next tab", "close tab", "reload the page", "zoom in", "select all", "copy", "paste", or "click Send" / "press the Save button". BYTE does it in the program in front. It uses PC control, so every step is logged, **Stop** works, passwords and card boxes are never clicked, and a click on a risky button (pay, buy, delete) still asks first. If two buttons fit the name, BYTE says so and does nothing: say more of the name.

## Read aloud with highlighting
Select text in any app and press the shortcut (Ctrl+Alt+L). A small window opens with the text and highlights the sentence being spoken. Choose **Slow**, **Normal** or **Fast** in the settings. Press the shortcut again, or **Stop**, to end it. It uses one of BYTE's downloaded voices (Settings → Voice and Hey BYTE); it will not press keys in a terminal or code editor or over a full-screen game.

## Live captions and "What did they just say?"
Right-click BYTE's tray icon and choose **Start live captions**. BYTE listens to the sound **this computer plays** (not your microphone), writes captions in a small window every few seconds, and keeps only the last 30 seconds, in memory. Choose **What did they just say?** to turn those 30 seconds into words. Nothing is saved, and it is off until you start it. It needs a speech model (Settings → Models → Voice); a large model makes captions lag, so a smaller one feels better.

## Your own wake phrase
Under **Your own wake phrase**, type what you want to say instead of "Hey BYTE" (one to three words, six letters or more, like "hey jarvis"). Leave it empty to keep "Hey BYTE". BYTE finds the phrase in what the speech model heard and forgives one wrong letter in a long word. BYTE has no special keyword model for custom phrases, so a custom phrase is less sure than "Hey BYTE": a longer, unusual phrase works better than a short common word, and you may get a miss or a false trigger now and then.
<!-- all -->
