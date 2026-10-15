# Slack and email

## Slack (your own app)
In [Settings → Control and screen](byte-setting:control), **Slack (your own app)** → **Set up**:
1. Create an app for your workspace at api.slack.com/apps (From scratch).
2. Under **OAuth & Permissions** add the bot scopes `channels:history`, `channels:read`, `chat:write` (and `groups:history`, `groups:read` for private channels).
3. **Install to Workspace** and copy the **Bot User OAuth Token** (it starts with `xoxb-`). Paste it into BYTE.
4. In Slack, add the app to each channel BYTE should use. Then **Find channels** in BYTE and tick them.

If BYTE says the app "isn't in that channel", add it from the channel's Integrations menu.

## Email (read, and draft replies)
**Email** uses IMAP, which almost every mail service offers:
1. Switch IMAP on in your mail settings.
2. Make an **app password** (never use your normal one) and enter the server (Gmail: `imap.gmail.com`), your address and that password.
3. Pick the folder to read, usually INBOX, and the folder for drafts.

BYTE **only reads** the folder: it does not mark mail as read, move it or delete it. The password lives in the credential store.

**Replies are drafts.** With **Reply for me** on, BYTE writes a reply for each new mail and shows it. If you say yes it is saved in your Drafts folder, and you can open it in your mail app (**Open in my mail app**) to send it. BYTE never sends email itself.

Mail is read every two minutes while BYTE is open. Long mails are cut to their first part, and quoted older mail is left out. Mail is outside text like any other message: BYTE never follows instructions written in it.

More: [Chat apps and email](help:chat-apps-and-email), [Jobs and triggers](help:jobs-and-triggers). Calendar links work as before under Connectors and appear in the [morning digest](help:morning-digest).
