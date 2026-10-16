# Chat apps and email (connectors)

BYTE can read and post in Discord, Telegram and Slack, and read your email, using **your own** bot, app or mail account. Every one is **off until you set it up and turn it on**: [Settings → Control and screen](byte-setting:control), under **Chat apps and email**.

## How it is kept safe
- **Your own token.** You make the bot or app yourself and paste its token. The token goes into the system's credential store (its own secret store, never a file), never into a file or a log. Nothing is shared with anyone and nothing runs on anyone else's computer.
- **A list of channels.** BYTE only reads from, and posts to, the channels you tick. Anything else is ignored.
- **Limits you choose.** Reads per minute and posts per hour, each with an **Unlimited** choice.
- **A log without words.** *What connectors did* lists times, channels and counts. It never contains a message.
- **Messages are information, not orders.** Whatever someone writes is handed to BYTE fenced off as outside text. If a message says "ignore your rules" or "run this command", BYTE does not.
- **Never in kids mode**, and nothing is read while the offline switch is on.
- Messages that were already there when you turned a connector on are not read back to you.

## Reply for me
Turn on **Reply for me** for a connector and BYTE drafts a reply to each new message in an allowed channel. The draft appears under **Waiting for your yes**. You can send it, edit it first, or say no. Nothing is posted without your yes.

If you want, name **one** channel for auto-reply. BYTE then answers there without asking, but never faster than the limit you set. Every other channel keeps asking.

## Which connector to use
- [Discord](help:discord): your own bot, or a webhook that can only post.
- [Telegram](help:telegram): your own bot, and asking your computer from your phone.
- [Slack and email](help:slack-and-email).
- Use any of them in a [job](help:jobs-and-triggers) or the [morning digest](help:morning-digest).

If a connector shows an error, the message says what to fix (a wrong token, a bot that was not added to the channel, a missing permission). The most common one: Discord sends empty messages until **Message Content Intent** is switched on in the Developer Portal.
