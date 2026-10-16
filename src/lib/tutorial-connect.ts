// Tutorial lessons for chat apps, email, jobs and the morning digest. Registered in tutorial.ts with one line.
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const CONNECT_TUTORIALS: Tutorial[] = [
  {
    id: "chat-apps",
    title: "Connect Discord, Telegram or Slack",
    promise: "Let BYTE read and post in the channels you choose, with your own bot",
    minutes: 6,
    icon: "chat",
    offByDefault: true,
    steps: [
      { id: "idea", title: "Your own bot, your own token", body: ["BYTE uses a bot or app that you make yourself. Its token goes into your computer's credential store, never into a file, and nothing is shared with anyone.", "Every connector is off until you set it up and turn it on."], links: [H("chat-apps-and-email", "Read how it is kept safe")] },
      { id: "setup", title: "Follow the numbered steps", body: ["Open Settings, Control and screen, and scroll to Chat apps and email. Press Set up on Discord, Telegram or Slack. The steps have the links you need.", "For Discord, switch Message Content Intent on, or Discord sends empty messages.", "BYTE never logs in to a personal Discord account. Only a bot or a webhook."], links: [S("control", "Open Chat apps and email"), H("discord", "Discord steps")], ifStuck: "If Check says the token is wrong, copy it again. Discord shows a token only once; Reset Token makes a new one." },
      { id: "check", title: "Check the token and tick channels", body: ["Paste the token and press Check and save. BYTE asks the service who the bot is.", "Press Find channels and tick only the channels BYTE may use. Anything else is ignored."], ifStuck: "No channels? Add the bot to your server (Discord) or the app to the channel (Slack) first." },
      { id: "on", title: "Turn it on and test", body: ["Switch the connector on, then press Send a test message. Set the reads per minute and posts per hour, or choose Unlimited.", "Turn it on only when you want it: off means BYTE reads and posts nothing."] },
      { id: "reply", title: "Reply for me", body: ["Turn on Reply for me and BYTE drafts a reply to each new message. You see it under Waiting for your yes and can send, edit or refuse it. Nothing is posted without your yes."], links: [H("chat-apps-and-email", "More about Reply for me")] },
    ],
  },
  {
    id: "phone-ask",
    title: "Ask your computer from your phone",
    promise: "Send a question or start a saved routine from Telegram",
    minutes: 4,
    icon: "bolt",
    offByDefault: true,
    steps: [
      { id: "bot", title: "Make a Telegram bot", body: ["Talk to BotFather in Telegram, send /newbot, and paste the token into BYTE under Chat apps and email. Then send your bot a message and press Find chats in BYTE."], links: [S("control", "Open Chat apps and email"), H("telegram", "Telegram steps")] },
      { id: "chat", title: "Tick only your private chat", body: ["Tick your own private chat with the bot. Group chats are ignored, because anyone in a group could type a command."] },
      { id: "ask", title: "Turn on asking from the phone", body: ["Turn on Ask the computer from the phone. Then a message can ask a question, or start a saved routine: send run game time. The answer comes back to your chat.", "Ask me on the computer before each one makes every request wait for your yes."], example: "run game time" },
      { id: "control", title: "Control has its own switch", body: ["Operating your computer from Telegram is off until you turn on Allow control from Telegram. Even then it follows the usual computer-control rules and approvals, and the result is reported back.", "Leave it off if you only want answers and routines."], ifStuck: "If BYTE says control is off, that is the switch working." },
    ],
  },
  {
    id: "jobs",
    title: "Jobs, triggers and the morning digest",
    promise: "Have BYTE do something on a schedule or when something happens",
    minutes: 5,
    icon: "clock",
    offByDefault: true,
    steps: [
      { id: "new", title: "Make a job", body: ["Open Settings, Control and screen, Jobs and triggers, and press New job. Give it a name and say what it should do in plain words.", "Every job starts off."], links: [S("control", "Open Jobs and triggers"), H("jobs-and-triggers", "About jobs")], example: "Summarise today's news about the Rust language in five lines." },
      { id: "when", title: "Pick when it runs", body: ["A time of day, weekdays, or every few hours. Or a trigger: a download finishes, a file appears in a folder, a new message arrives in an allowed channel, a watched page changes, or BYTE starts."] },
      { id: "tools", title: "Choose what it may use", body: ["Tick only what the job needs: the web, your files, memory or the browser. A job runs with nobody watching, so a step that needs your approval is not done."] },
      { id: "run", title: "Run it and read its history", body: ["Press Run now. The result is saved as a chat. History shows when it ran and what it did, with a Rerun button. Then turn the job on with its checkbox.", "Pause all jobs stops everything with one switch."] },
      { id: "digest", title: "Read my digest aloud", body: ["Press Add read my digest aloud for a daily spoken summary of your briefing, your news feeds and the channels you tick for it. It is off until you turn it on."], links: [H("morning-digest", "About the digest")] },
    ],
  },
];
