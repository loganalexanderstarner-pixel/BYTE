// Connectors (Discord, Telegram, Slack, email) and jobs: shared types and the plain helpers behind Settings. The screens are
// components/settings/ConnectSettings.tsx; the Rust side is src-tauri/src/messaging and src-tauri/src/triggers.

export type ConnectorKind = "discord" | "discord-webhook" | "telegram" | "slack" | "email";

export interface AllowedChannel {
  id: string;
  name: string;
}

export interface ConnectorRow {
  kind: ConnectorKind;
  name: string;
  hasToken: boolean;
  enabled: boolean;
  channels: AllowedChannel[];
  readsPerMin: number;
  postsPerHour: number;
  replyForMe: boolean;
  autoReplyChannel: string | null;
  autoReplyPerHour: number;
  phoneAsk: boolean;
  allowControl: boolean;
  confirmEach: boolean;
  inDigest: boolean;
  host: string;
  user: string;
  folder: string;
  draftsFolder: string;
}

export interface PendingItem {
  id: string;
  kind: "reply" | "phone";
  connector: ConnectorKind;
  channel: string;
  channelLabel: string;
  author: string;
  incoming: string;
  draft: string;
  created: number;
}

export interface ConnectStatus {
  rows: ConnectorRow[];
  pending: PendingItem[];
  jobsPaused: boolean;
}

export interface ChannelInfo {
  id: string;
  name: string;
  group: string;
}

export interface ConnectorPatch {
  enabled?: boolean;
  channels?: AllowedChannel[];
  readsPerMin?: number;
  postsPerHour?: number;
  replyForMe?: boolean;
  autoReplyChannel?: string | null;
  autoReplyPerHour?: number;
  phoneAsk?: boolean;
  allowControl?: boolean;
  confirmEach?: boolean;
  inDigest?: boolean;
}

export interface ActivityEntry {
  at: number;
  connector: string;
  action: string;
  channel: string;
  count: number;
  ok: boolean;
  note: string;
}

export type When =
  | { type: "daily"; time: string }
  | { type: "weekdays"; time: string }
  | { type: "everyHours"; hours: number }
  | { type: "download" }
  | { type: "folder"; path: string }
  | { type: "message"; connector: string; channel: string }
  | { type: "start" }
  | { type: "watcher"; id: number };

export interface JobTarget {
  connector: string;
  channel: string;
}

export interface AutoJob {
  id: number;
  name: string;
  instruction: string;
  when: When;
  enabled: boolean;
  tools: string[];
  needsPc: boolean;
  speak: boolean;
  postTo: JobTarget | null;
  template: "" | "digest";
  maxRunsPerDay: number;
  keepRuns: number;
  nextRun: number | null;
  lastRun: number | null;
  lastOk: boolean | null;
  lastChat: string | null;
  describe: string;
}

export interface JobRun {
  id: number;
  jobId: number;
  started: number;
  finished: number | null;
  ok: boolean | null;
  summary: string;
  chat: string | null;
  trigger: string;
}

export const TOOL_GROUPS: { id: string; label: string; hint: string }[] = [
  { id: "web", label: "Search the web and read pages", hint: "Needs the internet; off while the offline switch is on." },
  { id: "files", label: "Search my files", hint: "Looks only in the folders you added to My files." },
  { id: "memory", label: "Remember things", hint: "May save a note to BYTE's memory." },
  { id: "browser", label: "Use the browser", hint: "Opens pages and clicks for you." },
];

/** The "how often" choices; 0 is Unlimited. */
export const PER_MINUTE = [1, 6, 12, 30, 0];
export const PER_HOUR = [5, 20, 60, 200, 0];
export const PER_DAY = [1, 4, 24, 100, 0];
export const KEEP = [10, 50, 200, 0];

export const countLabel = (n: number, unit: string): string => (n === 0 ? "Unlimited" : `${n} per ${unit}`);
export const keepLabel = (n: number): string => (n === 0 ? "Unlimited" : `Last ${n}`);

export function describeWhen(w: When): string {
  switch (w.type) {
    case "daily":
      return `Every day at ${w.time}`;
    case "weekdays":
      return `Every weekday at ${w.time}`;
    case "everyHours":
      return w.hours === 1 ? "Every hour" : `Every ${w.hours} hours`;
    case "download":
      return "When a download finishes";
    case "folder":
      return `When a new file appears in ${w.path || "a folder"}`;
    case "message":
      return `When a new message arrives (${w.connector}${w.channel ? ", one channel" : ", any allowed channel"})`;
    case "start":
      return "When BYTE starts";
    case "watcher":
      return "When a watched page changes";
  }
}

export const WHEN_CHOICES: { id: When["type"]; label: string }[] = [
  { id: "daily", label: "Every day at a time" },
  { id: "weekdays", label: "Weekdays at a time" },
  { id: "everyHours", label: "Every few hours" },
  { id: "download", label: "When a download finishes" },
  { id: "folder", label: "When a file appears in a folder" },
  { id: "message", label: "When a message arrives in an allowed channel" },
  { id: "watcher", label: "When a watched page changes" },
  { id: "start", label: "When BYTE starts" },
];

export function defaultWhen(type: When["type"]): When {
  switch (type) {
    case "daily":
    case "weekdays":
      return { type, time: "08:00" };
    case "everyHours":
      return { type, hours: 3 };
    case "folder":
      return { type, path: "" };
    case "message":
      return { type, connector: "discord", channel: "" };
    case "watcher":
      return { type, id: 0 };
    default:
      return { type } as When;
  }
}

export function blankJob(): AutoJob {
  return { id: 0, name: "", instruction: "", when: { type: "daily", time: "08:00" }, enabled: false, tools: ["web"], needsPc: false, speak: false, postTo: null, template: "", maxRunsPerDay: 24, keepRuns: 50, nextRun: null, lastRun: null, lastOk: null, lastChat: null, describe: "" };
}

/** The form's own check, so the person sees the problem before saving. */
export function jobProblem(j: AutoJob): string | null {
  if (!j.name.trim()) return "Give the job a name.";
  if (j.template === "" && !j.instruction.trim()) return "Say in a few words what the job should do.";
  const w = j.when;
  if ((w.type === "daily" || w.type === "weekdays") && !/^\d{2}:\d{2}$/.test(w.time)) return "Pick a time.";
  if (w.type === "everyHours" && !(w.hours >= 1 && w.hours <= 168)) return "Pick 1 to 168 hours.";
  if (w.type === "folder" && !w.path.trim()) return "Pick a folder to watch.";
  if (w.type === "watcher" && !(w.id > 0)) return "Pick a watched page.";
  return null;
}

export const TRIGGER_LABEL: Record<string, string> = { schedule: "On schedule", manual: "By hand", download: "A download finished", folder: "A new file", message: "A new message", start: "BYTE started", watcher: "A page changed" };

/** "Skipped: ..." history rows were not run. */
export const wasSkipped = (r: JobRun): boolean => r.summary.startsWith("Skipped:");

// ---------------------------------------------------------------- the setup steps

export interface WizardStep {
  text: string;
  link?: { label: string; url: string };
}

export interface Wizard {
  title: string;
  /** What the pasted secret is called. */
  secretLabel: string;
  steps: WizardStep[];
  /** Why this way and not another. */
  note?: string;
}

export const WIZARDS: Record<Exclude<ConnectorKind, "email">, Wizard> = {
  discord: {
    title: "Discord (your own bot)",
    secretLabel: "Bot token",
    steps: [
      { text: "Open the Discord Developer Portal and choose New Application. Name it anything (for example \"My BYTE\").", link: { label: "Open the Developer Portal", url: "https://discord.com/developers/applications" } },
      { text: "In the application, open the Bot page. Under Privileged Gateway Intents, switch Message Content Intent ON and save. Without it Discord sends BYTE empty messages." },
      { text: "Still on the Bot page, press Reset Token and copy the token. It is shown once. Paste it below." },
      { text: "Open OAuth2, then URL Generator. Tick the scope bot and the permissions View Channels, Read Message History and Send Messages. Open the link it makes and add the bot to your server." },
      { text: "Back here: paste the token, press Check, then Find channels and tick the ones BYTE may use." },
    ],
    note: "BYTE only uses a bot you made, or a webhook. It never logs in to a personal Discord account: that is against Discord's rules and can get the account banned.",
  },
  "discord-webhook": {
    title: "Discord webhook (post only)",
    secretLabel: "Webhook address",
    steps: [
      { text: "In Discord, open the channel's settings (the gear), then Integrations, then Webhooks, then New Webhook." },
      { text: "Press Copy Webhook URL and paste it below. BYTE can then post to that one channel. It cannot read anything." },
    ],
    note: "The easy way to get results into Discord: no bot, no intents. Anyone who has this address can post to the channel, so BYTE keeps it in the credential store only.",
  },
  telegram: {
    title: "Telegram (your own bot)",
    secretLabel: "Bot token",
    steps: [
      { text: "In Telegram, open a chat with BotFather.", link: { label: "Open BotFather", url: "https://t.me/BotFather" } },
      { text: "Send /newbot, choose a name and a user name ending in bot. BotFather replies with a token like 123456:ABC-DEF. Paste it below." },
      { text: "Open your new bot in Telegram and send it any message (for example \"hi\"). Bots only see chats that started with them." },
      { text: "Back here: press Check, then Find chats, and tick your own private chat with the bot." },
    ],
    note: "Only tick your own private chat. Group chats are never used to ask your PC for things.",
  },
  slack: {
    title: "Slack (your own app)",
    secretLabel: "Bot User OAuth Token (xoxb-...)",
    steps: [
      { text: "Create a Slack app for your workspace (From scratch).", link: { label: "Open Slack apps", url: "https://api.slack.com/apps" } },
      { text: "Open OAuth & Permissions and add these Bot Token Scopes: channels:history, channels:read, chat:write (and groups:history, groups:read for private channels)." },
      { text: "Press Install to Workspace, then copy the Bot User OAuth Token (it starts with xoxb-) and paste it below." },
      { text: "In Slack, open each channel BYTE should use and add the app to it. Then press Check and Find channels here." },
    ],
  },
};

export const EMAIL_STEPS: WizardStep[] = [
  { text: "Turn on IMAP in your mail account's settings (Gmail: Settings, See all settings, Forwarding and POP/IMAP)." },
  { text: "Make an app password (Gmail: your Google account, Security, App passwords). Use it here, never your normal password." },
  { text: "Enter the server (Gmail: imap.gmail.com), your address and the app password, then press Check and save." },
  { text: "Pick the folder to read. BYTE only reads it; it never deletes or moves mail. Replies are saved as drafts for you to send." },
];

/** Adds or removes a channel from the allowed list. */
export function toggleChannel(list: AllowedChannel[], c: AllowedChannel): AllowedChannel[] {
  return list.some((x) => x.id === c.id) ? list.filter((x) => x.id !== c.id) : [...list, { id: c.id, name: c.name }];
}

/** Channels grouped by server / workspace, in the order given. */
export function groupChannels(cs: ChannelInfo[]): { group: string; channels: ChannelInfo[] }[] {
  const out: { group: string; channels: ChannelInfo[] }[] = [];
  for (const c of cs) {
    const g = out.find((x) => x.group === c.group);
    if (g) g.channels.push(c);
    else out.push({ group: c.group, channels: [c] });
  }
  return out;
}
