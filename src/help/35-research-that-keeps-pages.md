# Research that keeps its pages

<!-- mac -->
This is part of the Windows app for now. It isn't in the Mac app yet, so there is nothing to set up here. See the other help articles for what the Mac app can do.
<!-- linux -->
This isn't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
Three options in [Settings → Control and screen](byte-setting:control) → **Browser research**. All three are **off until you turn them on**, and none of them works in a private chat.

## Keep the pages BYTE opened for me
Needs **Watch BYTE browse** (so the browsing window is visible). When BYTE finishes researching, its window **stays open** with a note that the pages are yours to read, instead of closing. The answer ends with **Pages I opened**: a list of links, saved in the chat. Later you can say "summarise what you read" or "summarise those pages", and BYTE opens those links again and answers from them.

The browsing window is one window, not many tabs. The next time BYTE browses, it closes the old window and opens a fresh one, so the chat's list of links is what lasts.

## Keep researching until I'm back
Choose **15 minutes, 1 hour, 3 hours, 8 hours or Unlimited**. When you ask BYTE to research something, it reads the first pages as usual and then keeps going, round after round, with more searches from new angles (news, reviews, problems, comparisons and so on).

**You watch it happen.** BYTE opens a visible **Edge** window (or Chrome if Edge is missing) with a separate profile, and every page it reads opens in its **own new tab**. The usual rules still apply to each one: your site lists, no banking sites, no private fields, approval cards, the activity log.

**Ending it.** The job stops when:
- you say **"I'm back"**: type it in the chat, say it (with "Hey BYTE" on), or press **I'm back** on the live card that shows while it runs,
- you press **Stop**,
- the **time limit** you chose is up (Unlimited never stops for time), or
- you close the Edge window.

When all the planned searches are done and nothing has stopped it, BYTE **stands by** with the tabs open until one of those happens.

**What you get.** BYTE writes the summary from the pages it read and lists them as links. By default it then **leaves the pages open for you** in a normal Edge window. Turn on **Close the tabs when I'm back** to close them instead.

**The debugging port.** To open tabs and read them, BYTE starts Edge with a **debugging port**: a door that only this computer can use (address 127.0.0.1, a random number each time). It exists **only while a job runs**. When the job ends BYTE disconnects, closes that Edge, and (unless you chose to close the tabs) opens the pages again in a normal window that has **no debugging port**. Because that is a new window, pages you were signed in to inside BYTE's separate profile are shown by their address again, and a half-filled form is not kept.

If Edge or Chrome can't be started, BYTE researches in the background instead, and stops when you come back to the keyboard or mouse.

## Use my own Edge profile
Normally BYTE uses a **separate profile** it made, with none of your sign-ins. This option uses **your everyday Edge profile** instead, with your logins and history. **Close Edge first.** BYTE still asks before it submits, buys, sends, deletes or downloads anything, never types passwords or card numbers, and blocks banking sites. Newer Edge versions may refuse to be controlled this way; then BYTE says nothing is wrong and uses its own separate profile.

You are asked to confirm before it turns on.
<!-- all -->
