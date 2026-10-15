# Planning: accuracy, notes and dry run

<!-- mac -->
This is part of the Windows app for now. It isn't in the Mac app yet, so there is nothing to set up here. See the other help articles for what the Mac app can do.
<!-- linux -->
This isn't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
When BYTE plans a job on your PC, it can check the plan against the window in front before it does anything. The settings are in [Settings → Control and screen](byte-setting:control) → **Planning**. They show once **PC control** or **Guided help** is on.

## What is always on
- **Checked against the window:** BYTE only clicks buttons that are really in the list it was shown, only switches to windows that are open, and fixes or sends back a plan that does not match. If a plan is too long for your step limit, the model is asked to shorten it. The exact problem is given to the model on the retry.
- **Strict format:** the model is asked to write only valid steps. If it can't, BYTE asks for plain JSON as before.
- **Saying no:** when a job needs a password or card number, BYTE says it can't instead of guessing.

## Where plans are made
- **Plan with BYTE Cloud** is off until you turn it on. Turning it on asks you once. It sends your request and the names of the buttons and boxes in the window in front. It sends no picture, no typed text from the window and nothing from password or card fields, and never in private chats, kids mode, BYTE's own window or programs you excluded.
- **Ask where** is **Local** until you change it. **Cloud** uses BYTE Cloud. **Auto** uses this PC's model unless it is too small to plan reliably. Cloud and Auto do nothing while cloud planning is off. This applies to PC control and to guided help.

## Test a model
**Models and speed → PC control** lists the models that plan well, the recommended one first. **Test** runs 40 standard tasks (open a program, switch a window, type, click a named button, change a setting, find a file, several steps, and jobs that should be refused) and shows a score. Nothing is clicked or typed. **Use for PC control** picks the model, and downloads it first if needed. You can also run the test from Settings → Planning.

## Notes about programs
Off until you turn on **Notes about programs**. After a job works, BYTE saves the names of the buttons it used (never what it typed) and uses them as a hint next time. Notes stay on this PC. You can add your own ("in Notepad the font is under the gear"), delete one, or clear all.

**Learn from corrections** is a second switch. After you stop a job and carry on in another program, BYTE offers to save a note. It always asks, and never saves by itself. A job that failed also offers a note.

## Dry run
Turn on **Dry run**, or start a message with "dry run:" (for example "dry run: open Notepad and type hello"). BYTE lists what it would click, type and press, and does none of it.
<!-- all -->
