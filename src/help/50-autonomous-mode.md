# Autonomous mode: jobs that run by themselves

<!-- mac -->
This is part of the Windows app for now. It isn't in the Mac app yet, so there is nothing to set up here. See the other help articles for what the Mac app can do.
<!-- linux -->
This isn't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
Autonomous mode lets BYTE work through a list of stages on its own while you are away, then write you a report. It is **off until you turn it on** and accept a notice: [Settings → Control and screen](byte-setting:control) → **Autonomous mode**. It needs **Let BYTE use this PC for you** to be on, and it never runs in kids mode.

## A job, step by step
1. Write the job in your own words under **New job**, then press **Split into stages**. BYTE shows the list. Edit it, remove stages, then check it: nothing starts before you press **Yes, start the job**. A numbered list you write yourself is used as it is.
2. Set the **run contract** (below).
3. Press **Yes, start the job**. A small always-on-top window shows what BYTE is doing now, what comes next, a progress bar and a big **Stop** button. A card in the chat window shows the same, with a live list of steps.
4. When the job ends, or you stop it, BYTE saves a written **report** as a chat and can read a short version aloud.

## The run contract
You decide, before it starts:
- **Programs** it may use ("browser" is BYTE's own browser) and **folders** it may read from or write into.
- **People** it may message (only through connectors you have set up; none if empty).
- **Never delete anything** (on by default) and **never spend money** (always on).
- A **time**, **step** and **message** budget. Every one has an **Unlimited** choice.

Anything outside the contract **pauses the job and waits for you**: allow it once, or skip it. Deleting, spending, and programs on your never-touch list are refused, not asked about. Under **Never ask, just do it** BYTE does not pause for things outside the contract, but those refusals and the budgets still hold.

## Stopping and pausing
- **Stop** on the small window, the **stop key** (default Ctrl+Alt+F12, change it in the same panel; it only exists while a job runs), or Stop in BYTE. Stop always works.
- **Pause when I use the PC** (off by default) pauses the job if you move the mouse or press keys. It is a best effort.
- While a job runs the PC and screen stay awake; they sleep normally afterwards.

## Closing BYTE in the middle
Each stage is saved when it ends. If BYTE closes, the job waits under **Jobs** with a **Resume** button and continues at the first stage that is not finished (a stage that was running starts over). A job that stopped on a budget can be resumed with no limit on that budget.

## What it keeps
**Audit trail of autonomous jobs** lists every step with its time, program and result, and can be exported. What was typed is never kept, only how much. Pages and messages BYTE reads are treated as information, never as instructions.
