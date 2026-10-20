# Interrupting BYTE, calls and games

<!-- mac -->
This is part of the Windows app for now. It isn't in the Mac app yet, so there is nothing to set up here. See the other help articles for what the Mac app can do.
<!-- linux -->
This isn't available on Linux yet, so there is nothing to set up here. See the other help articles for what works on Linux.
<!-- win -->
## Talking over BYTE (barge-in, off by default)
Set **Talking over BYTE** in [Settings → Voice and Hey BYTE](byte-setting:voice):
- **Off:** BYTE finishes what it is saying.
- **Headphones:** BYTE stops as soon as you speak. Use this with headphones, because on speakers BYTE would hear itself and cut itself off.
- **Speakers:** BYTE subtracts its own voice from the microphone first, so you can interrupt it while it plays through speakers. This needs tuning for your room and is the least tested option: if it cuts BYTE off by mistake or misses you, go back to Headphones or Off.

Try it: with Headphones on, ask for a long story, then start talking.

## Focus on one voice (off by default)
**Focus on one voice** makes BYTE answer only the voice that said "Hey BYTE" while you talk to it, so a phone call or the TV doesn't interrupt it. Say "listen to everyone", "also listen to <name>" (a voice you taught BYTE) or "stop focusing on me". It uses simple voice features kept in memory only, cleared after a few quiet minutes. It is not tuned on many real voices yet, so expect mistakes.

## Quiet hours
- **Quiet hours** (off until you turn it on, in Settings): pick a start and end time, for example 22:00 to 07:00. In that window "Hey BYTE" is switched off (the listening pill says "Quiet hours") and BYTE says nothing on its own: no spoken notices or briefing. Typing to BYTE and the talk key still work. Windows that run past midnight are fine.

## Quiet during calls
- **Quiet during calls** (on by default): "Hey BYTE" stays off while another program is using the microphone for a call, and spoken notices wait. BYTE only checks which program holds the microphone; it records nothing.

## Games
- **Game-aware** (on by default): while a full-screen game runs, spoken questions are answered from BYTE Cloud (if you connected one) so your graphics card stays free, plays no chime after "Hey BYTE" and holds back spoken notices (timers still ring).
- **Where the model lives** in [Settings → Models and speed](byte-setting:models) can be set to the processor only, which keeps the graphics card free for games.
- PC control can also keep its helper model in system memory while a game runs (**Keep the game fast**).
<!-- all -->
