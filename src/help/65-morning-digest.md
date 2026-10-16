# The morning digest

"Read my digest aloud" is a ready-made job: at a time you choose, BYTE puts together
- your **briefing** (calendar, reminders, to-dos, weather, headlines on topics you follow),
- the **news feeds** you follow, and
- a short summary of what was said in the Discord, Telegram, Slack and email channels **you allowed for it**,

saves it as a chat, and reads it out loud.

## Turn it on
1. In [Settings → Control and screen](byte-setting:control), **Jobs and triggers**, press **Add "read my digest aloud"**. It is created off at 07:30.
2. Under each connector you want in it, tick **Include these channels in the morning digest**. Connectors you leave out are not read for it.
3. Edit the job to change the time, then turn it on.

## Good to know
- It speaks only when speech is on, and never during a game or a call. The text is always in the chat it saves.
- Only the channel summary is written by the model, and it sees the messages as outside text without any tools. Calendar and feeds are copied, not invented.
- Messages from Telegram are kept in BYTE's memory only until it closes, so the digest after a restart may lack them. Discord, Slack and email keep their own history and are looked at when the digest runs.
- With the offline switch on, the feeds and channels are skipped and the rest is still read.

See also [Jobs and triggers](help:jobs-and-triggers) and [Chat apps and email](help:chat-apps-and-email).
