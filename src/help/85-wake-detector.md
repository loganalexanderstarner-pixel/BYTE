# The wake word detector

<!-- mac -->
This is part of the Windows app for now. It isn't in the Mac app yet, so there is nothing to set up here. See the other help articles for what the Mac app can do.
<!-- linux -->
This isn't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
"Hey BYTE" can be spotted in two ways. You choose in [Settings → Voice and Hey BYTE](byte-setting:voice), under **Wake word detector**. It only matters while **Listen for "Hey BYTE"** is on, and that switch is off until you turn it on.

## Careful and Quick
- **Careful (speech model only):** the speech model reads every burst of speech and looks for the phrase. This is how it always worked. It is the most sure way, and it uses the most processor time, because every sentence said near the microphone is read.
- **Quick (new):** a very small detector, about a hundred kilobytes, listens to every moment of sound. When it thinks it heard "Hey BYTE", the speech model reads that burst to make sure. Speech the detector does not like is not read at all, so the processor rests while you talk about other things.

The detector never wakes BYTE on its own. A guess costs one extra check by the speech model; the speech model keeps the final say.

## Also while BYTE is talking
With Quick on and **Talking over BYTE** set to **Speakers**, the detector listens to what is left of the microphone after BYTE's own voice is taken out. If it hears "Hey BYTE" there, the speech model checks it, and if it agrees BYTE stops talking and listens. This depends on the echo canceller; see [Interrupting BYTE, calls and games](help:interrupting-calls-games). With headphones or when talking over BYTE is off, nothing changes.

## What you should know
- **It was trained on computer-made voices.** There was no recording of real people to train on, so the detector can miss a real voice more often than the speech model does. If you have to say "Hey BYTE" twice, switch back to **Careful**.
- **The test log shows what it thinks.** Turn on the Test log in Voice and Hey BYTE. Each guess is a line starting with "detector", with its score from 0 to 1, and each burst it skipped is listed too. The log records words that were heard, so leave it off the rest of the time.
- **A wake phrase of your own uses Careful.** If you chose [your own phrase](help:voice-helpers), the detector is not used.
- **Nothing is recorded.** The detector keeps the last three seconds of sound in memory only, to hand to the speech model, and drops it as it goes. It is off in a call when "quiet during calls" is on.
<!-- all -->
