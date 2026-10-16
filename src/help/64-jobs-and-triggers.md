# Jobs and triggers

A **job** is something you ask BYTE to do in plain words, saved with a name, and run on a schedule or when something happens. You make them in [Settings → Control and screen](byte-setting:control) under **Jobs and triggers**. Every job is created **off**, and each belongs to your profile.

## When a job runs
- **On a schedule:** every day at a time, on weekdays at a time, or every few hours.
- **A download finishes:** a new, complete file appears in your Downloads folder (half-finished downloads are ignored).
- **A file appears** in a folder you pick.
- **A new message** in an allowed channel of a connector you turned on.
- **A page you watch changes** (use the page watchers you already have).
- **BYTE starts.**

BYTE has to be open for a job to run. A scheduled run missed while it was closed happens once at the next start.

## What a job may do
Each job has its own list of what it may use: search the web, search your files, remember things, use the browser. A job that only writes from what it is given needs none of them. A job that asks BYTE to operate the PC is marked **PC**, runs **only while PC control is on**, and follows its usual approvals. Jobs run with nobody watching, so a step that needs your approval is not done.

What a trigger hands to the job (a file name, a message) is fenced as outside text. A message cannot give the job new orders.

## Control
- **Pause all jobs** is one switch for everything. **Run now** still works while paused.
- **Runs per day** and **History** each have an **Unlimited** choice.
- Each run lands in the job's **History**: when it ran, what it did, and whether it finished. A skipped run says why ("PC control is off", "all jobs are paused"). **Rerun** runs it again.
- A result is saved as a chat and shown as a notification. A job can also say it out loud, or post it to a channel you allowed.
- Nothing runs in kids mode.

## Try it
1. Press **New job**, name it "Rust news", and write "Summarise today's news about the Rust language in five lines."
2. Pick **Every day at** 08:00, leave **Search the web** ticked, and save.
3. Press **Run now**, then open **History**.
4. Turn the job on with its checkbox.
