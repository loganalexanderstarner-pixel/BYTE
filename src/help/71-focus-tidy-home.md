# Focus mode, Downloads tidy-up, recipes, Home Assistant and game help

<!-- mac -->
**Downloads tidy-up** and **Home Assistant** work on the Mac too (turn them on in [Settings → Control and screen](byte-setting:control) under **Everyday helpers**). Focus mode, recipes and the game helpers are part of the Windows app for now.
<!-- linux -->
These helpers aren't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
Find all of these in [Settings → Control and screen](byte-setting:control) under **Everyday helpers**. Each is **off until you turn it on**, belongs to you on this computer, and stays off in kids mode and private chats.

## Focus mode
Right-click BYTE's tray icon and choose **Start a focus session**. Type what you will work on (or leave it empty) and press **Start**. BYTE turns pop-up notifications off, says your plan out loud, and times the session. Every so often it nudges you to take a break, and at the end it tells you the time is up. Notifications come back when the session ends, when BYTE starts again, or after 12 hours at the latest, even if something goes wrong. **Session length** and **Break reminders** both have a "No limit" / "No reminders" choice. Choose **End the focus session** in the tray menu to stop early.

## Downloads tidy-up
When ten or more loose, finished files sit in your Downloads folder, a small popup offers to sort them into **Pictures, Documents, Installers, Archives, Music** and **Videos** folders inside Downloads. Files that are still downloading, were changed in the last ten minutes, or that BYTE does not recognise are left alone, and **nothing is ever deleted or moved out of Downloads**. After sorting, **Undo** puts every file back (even after a restart). You can also choose **Tidy my Downloads** in the tray menu to look right now.

## Recipes
A recipe is a routine saved as a small file you can share. Under **Recipes**, choose **Export** next to a routine to save it in Documents/BYTE/Recipes (no schedule is included). To add someone else's, paste its text and choose **Review it**: BYTE lists every step, marks the ones to look at twice (opening a web page, closing a program, running one of your automations), and refuses anything that breaks the routine rules (for example closing a part of Windows). You can change the phrase before you choose **Add it**. A recipe can only hold the few steps routines have; it can never run a script.

## Home Assistant
Type your Home Assistant's address (on your home network, like `http://homeassistant.local:8123`), make a long-lived access token in your Home Assistant profile, paste it, and choose **Test**. The token is stored in the system credential store and is never shown again. Then say or type "turn on the kitchen light" or "turn off the desk plug". BYTE only changes **lights, plugs (switches), fans and input switches**, never locks, alarms or garage doors, and it only acts when the name matches something Home Assistant has. If two things fit, it asks which one.

## Game companion
With **Remember the last seconds** on (under Looking at your screen), the tray menu offers **Review my last play**: BYTE looks back over the replay and says what seems to have gone wrong. **How is my game going?** looks once at the game and names the likely problems. Both use the model and the photo helper on this computer, so they work without internet. They say plainly what they cannot see.

## Notification pile
A summary of the notifications waiting in Windows is **not available**: Windows gives programs like BYTE no supported way to read other programs' notifications, so BYTE does not try.
<!-- all -->
