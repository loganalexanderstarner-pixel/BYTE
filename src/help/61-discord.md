# Discord

Two ways to use Discord with BYTE. Both use things Discord provides for this: a bot you make, or a webhook.

## Your own bot (read and post)
Open [Settings → Control and screen](byte-setting:control), find **Discord (your own bot)** and press **Set up**. The steps are numbered there, with the links:
1. In the Discord Developer Portal, make a **New Application**.
2. On the **Bot** page, switch **Message Content Intent** on and save. Without it Discord sends BYTE empty messages.
3. Press **Reset Token**, copy it, and paste it into BYTE. It is shown only once.
4. Under **OAuth2 → URL Generator** tick **bot**, then **View Channels**, **Read Message History** and **Send Messages**. Open the link it makes and add the bot to your server.
5. In BYTE press **Check and save** (BYTE asks Discord who the token belongs to), then **Find channels**, and tick the ones BYTE may use.
6. Turn the connector **on**. Press **Send a test message** to see it work.

BYTE looks at the ticked channels every half minute while it is open. It does not stay connected in the background and it does not need a server of its own.

## A webhook (post only)
The simplest way to get results into Discord. In a channel's settings choose **Integrations → Webhooks → New Webhook → Copy Webhook URL**, and paste it under **Discord webhook (post only)**. BYTE can then post to that one channel. It can not read anything. Treat the address like a password: anyone who has it can post.

## Why never a personal account
BYTE will not log in as you. Discord's rules forbid automating a personal account ("self-bots"), and accounts that do it get banned. A bot or a webhook is the allowed way. If a token you paste belongs to a person instead of a bot, BYTE says so and does not keep it.

## Using it
- Ask a [job](help:jobs-and-triggers) to post its result to a channel, or start a job when a message arrives.
- Turn on [Reply for me](help:chat-apps-and-email) to get drafted replies for a yes.
- Include a channel in the [morning digest](help:morning-digest).
