# Talking over BYTE

<!-- mac -->
**Talking over BYTE** works on the Mac too, with headphones or speakers: choose it in [Settings → Voice and Hey BYTE](byte-setting:voice) under **Talking over BYTE**. The sound-card copy described below is part of the Windows app for now; on a Mac BYTE uses its own playback to remove its voice from the microphone.
<!-- linux -->
Talking over BYTE isn't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
You can interrupt BYTE while it speaks, and say "Hey BYTE" over it, with ordinary speakers and one microphone. Everything here is **off until you turn it on**, belongs to you on this computer, and never runs in private chats or kids mode.

## Turn it on
1. Open [Settings → Voice and Hey BYTE](byte-setting:voice) and turn on **Hey BYTE**.
2. Under **Talking over BYTE** choose **Headphones (any speech)** if you wear headphones or a headset, or **Speakers (echo cancelled)** if BYTE talks out of the speakers.
3. With **Speakers**, also turn on **Use the sound card's own copy of what the speakers play**. This is the exact signal Windows sends to the speakers, so BYTE can subtract its own voice (and any music playing) from the microphone far more precisely than with its own playback alone.

## What happens when you talk
- Start talking while BYTE speaks. After about a third of a second BYTE turns itself down, and after about four tenths of a second it stops and listens to you.
- Say **"Hey BYTE"** over BYTE and it stops at once and takes what follows as your new question ("Hey BYTE, what's the weather" stops BYTE and asks). The wake phrase itself is not part of the question.
- A cough or a short noise only turns BYTE down for a moment; it carries on.
- If more than one person is in the room, [focus on one voice](byte-setting:voice) lets only the person who said "Hey BYTE" interrupt.

## Tips for speakers
- BYTE needs a second or two of its own voice to learn your room. In that time it does not try to be interrupted. After that it remembers the room.
- Speak up a little and face the microphone. The nearer you are to the microphone compared with the speakers, the better it works.
- A very loud, distorting laptop speaker can be too much for any echo removal. Turn the volume down a notch, or use headphones.
- If BYTE keeps cutting itself off, choose **Headphones** off and **Speakers** on, or turn **Talking over BYTE** off.

## Checking what BYTE heard
Turn on **Test log** in [Settings → Voice and Hey BYTE](byte-setting:voice). Lines starting with "[test] interrupt" say why BYTE turned itself down or stopped, how loud your voice was against the leftover of its own, and how many times the microphone stream had to restart. The log records words that were spoken, so turn it off afterwards.

## If the microphone warns about "buffer underrun or overrun"
BYTE's microphone listener hands sound over without waiting, and restarts the stream by itself if Windows reports a problem. The Test log shows how many samples were dropped and how many restarts happened. A few over a whole day are normal; many in a minute mean another program is fighting for the microphone.
<!-- all -->
