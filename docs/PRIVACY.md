# Privacy, in plain words

This page says what stays on your device, what can leave it, and what each optional feature reads. It is written for people, not lawyers. If anything here ever differs from what BYTE does, that is a bug: please report it.

## The short version
- BYTE runs the model **on your device**. Your chats, files, notes and memories are processed and stored there.
- Chats, memories and settings are stored **encrypted** on your device. Keys and tokens live in the system's credential store (Windows Credential Manager, the Mac Keychain), never in files or logs.
- Anything that **listens, looks at your screen, controls your computer or talks to an outside service** is **off until you turn it on**, and each one has its own switch.
- Nothing is sent to anyone unless you choose BYTE Cloud or ask for something that needs the internet (a web search, a download).
- **Private chats** are not saved, do not use memories, never go to the cloud, and never use the screen, the browser or PC control. **Kids mode** turns the risky features off.

## What stays on your device
- Your chats, memories, notes, settings and the files you give BYTE.
- The sound from your microphone: it is turned into text on your device and is not kept.
- Screen pictures: any picture BYTE takes of a window is held in memory only, is never saved to disk, and is gone when the question is answered (the replay is gone when you quit, switch it off or open a private chat).
- The activity logs (what PC control did, what tools were used) and the daily summary built from them. Only counts and times appear in the summary.
- Problem reports and backups: these are files **you** choose to save. BYTE sends nothing by itself.
- The Never touch list, the approval history and every other setting. Settings are not shared between your devices unless you connect an account and the setting is one that follows you.

## What can leave your device
| What | When | Where it goes |
|---|---|---|
| A web search or a page you asked BYTE to read | Only when your question needs the web, and not when **Work offline** is on | The search service and the site you read |
| A model or voice download | When you download one | The place the file is hosted (checked against a fingerprint) |
| Update check | Once a day if **Check for updates** is on | BYTE's release page; the update must be signed by BYTE's key |
| Your messages and chosen details | Only if you connect **BYTE Cloud** and choose it for an answer | BYTE Cloud (invite only). Screen text stays local unless you choose Cloud for it |
| A message to a connector (chat apps, email, calendar) | Only for connectors you set up yourself, with your own token | That service |

Turn on **Work offline** (Settings → Privacy) and none of the above happens.

## What each optional feature reads
All of these are off until you switch them on.

| Feature | What it reads or does | Kept? |
|---|---|---|
| **Hey BYTE** | Listens through the microphone for the wake words. Short bursts of speech are checked on your device. The microphone icon of your system shows while it is on. | Sound is not kept |
| **Speak and listen for approvals** | BYTE says what it wants to do and listens for a spoken yes or no, in the same way as Hey BYTE | Not kept |
| **Screen questions** | When you ask, BYTE captures the window in front once, reads its text and answers | Picture in memory only |
| **Screen replay** | Keeps the last few seconds of a window in memory, with a visible badge. Stops for windows on the Never touch list | Memory only, never saved or sent |
| **PC control** | Reads the front window as a list of buttons and boxes, then clicks, types and opens things you asked for. Asks before risky steps. Never types passwords or card numbers. Refuses program files and scripts | Log of what was done; typed text is not logged, only how much |
| **Guided help** | Reads the front window while a guide runs and rings each button in turn, or does it for you | Not kept |
| **Browser** | Opens pages in a window you can watch and reads them. Follows your site lists. Blocks banking sites. Asks before it submits, buys, sends, deletes or downloads | Address and kind of step in the activity log |
| **Keep the pages BYTE opened** | Leaves the browsing window open and adds the list of pages to the chat as links | The links are saved in that chat |
| **Keep researching until I'm back** | Goes on searching and reading more pages in a visible Edge window, one new tab per page, until you say "I'm back", press Stop, or the limit you chose ends. Edge's debugging port (127.0.0.1 only, random number) is open only while the job runs | Same as the browser |
| **Use my own Edge profile** | Lets the browser feature use your everyday Edge with your sign-ins, instead of BYTE's separate profile. Strong warning, off by default | Same as the browser |
| **Laptop mode** | Asks the system whether you are on battery, to pause listening and the replay | Nothing |
| **Daily "what BYTE did" summary** | Counts and times from BYTE's own activity logs | Built on your device |
| **Clipboard history, Messages inbox, terminal, connectors** | As described in their own Settings text; each is off until you turn it on | On your device |

## The things that protect you
- **Never touch list** (Settings → Control and screen): password managers, banking, private browser windows and BYTE's own window start on it. PC control will not operate them and the screen features will not look at them.
- **Approval cards** for risky steps, with a time limit that ends in "no". Nothing is ever approved by itself.
- **Stop** always works, and a refusal of program files, scripts and dangerous key combinations cannot be switched off.
- **Safe mode** starts BYTE with all of the optional features off, without changing your settings.
- **Remove BYTE's data** (Settings → Backup and help) deletes what you choose before you uninstall. Your own notes and exported files are never touched.

## Your control
Every switch above is in [Settings](../README.md). The Activity list shows what BYTE did and can be cleared. **Erase everything** removes all chats, memories and settings. Nothing here depends on an account.

Questions or something that looks wrong? Open an issue on the project page.
