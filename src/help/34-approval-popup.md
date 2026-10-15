# Approval popup and spoken approvals

<!-- mac -->
This is part of the Windows app for now. It isn't in the Mac app yet, so there is nothing to set up here. On a Mac, BYTE's approval cards appear in the chat and always wait for your answer.
<!-- linux -->
This isn't available on Linux yet, so there is nothing to set up here. Approval cards appear in the chat and always wait for your answer.
<!-- win -->
When BYTE needs a yes before it does something (PC control, a click on a web page, sending a text), it shows a card in the chat. If BYTE's window is **not in front**, for example while you work in another program, the same question also appears as a **small popup** in the bottom-right corner of the screen you are using.

## The popup
- It shows what BYTE wants to do and where, with **Do it** and **Don't**.
- It stays on top but **never takes the keyboard**, so what you are typing goes where it was going. Clicking a button works normally.
- **Ctrl+Alt+Y** answers yes and **Ctrl+Alt+N** answers no from anywhere, only while a popup is showing.
- A counter shows how long is left. When the time runs out the answer is **no**. Nothing is ever approved by itself.
- If several questions are waiting, the newest is shown and the popup says how many more are behind it.

Turn the popup off in [Settings → General](byte-setting:general) → **Approval popup**. The card in the chat always works.

## Spoken approvals
Turn on **Speak and listen for approvals** (it needs [Hey BYTE](help:hey-byte) on). BYTE then says what it wants to do, for example "Open Notepad. Should I do it? Say yes or no.", and listens for your answer the same way it listens after "Hey BYTE", including the checks on whose voice it follows.
- Only a short, clear "yes" or "no" counts. Anything else makes BYTE ask once more, and then leave it to the buttons.
- You can still use the buttons or the keys at the same time; the first answer wins.

BYTE reads the question out loud, so turn this off if others can hear your speakers and the actions are private.

## If the popup does not appear
- BYTE's own window was in front: the card in the chat is already visible.
- The popup is off, or kids mode is on.
- Another program already uses Ctrl+Alt+Y or Ctrl+Alt+N: the buttons still work.
<!-- all -->
