# Telegram and asking your computer from your phone

## Set up your bot
Open [Settings → Control and screen](byte-setting:control), find **Telegram (your own bot)** and press **Set up**:
1. Open a chat with **BotFather** in Telegram and send `/newbot`. Pick a name and a user name that ends in "bot".
2. BotFather sends a token that looks like `123456:ABC-DEF`. Paste it into BYTE and press **Check and save**.
3. Open your new bot in Telegram and send it any message, such as "hi". A bot only sees chats that started with it.
4. In BYTE press **Find chats** and tick **your own private chat** with the bot. Turn the connector on.

BYTE reads new messages every few seconds while it is open. If you ever see "has a webhook set", that bot was used for something else; make a new one with BotFather.

## Ask the computer from the phone
Turn on **Ask the computer from the phone**. From your private chat you can then:
- send a question, and BYTE answers it with the local model and sends the answer back;
- send `run game time` to start one of your saved routines (`/list` shows their names), and get the result back.

This is **not** remote control. By default BYTE only answers questions and runs routines you already made. A message that asks BYTE to operate the PC ("close Chrome", "turn the volume down") is refused with a note, unless you also turn on **Allow control from Telegram**. That has its own switch, which is off, and even then PC control has to be on and its usual approvals and limits apply; steps that need an approval are not done when you are not at the PC, and the result is always sent back.

**Ask me on the computer before each one** makes every request wait for your yes in Settings first, and BYTE tells your phone it is waiting.

Only private chats with your bot count. Group chats are ignored, because anyone in a group could type a command. If a stranger finds your bot, they are not on your list and get no answer.

## Everything else
Limits, reply-for-me, jobs and the digest work as described in [Chat apps and email](help:chat-apps-and-email).
