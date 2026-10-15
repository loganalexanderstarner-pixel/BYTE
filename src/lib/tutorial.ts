// The tutorial: one short walkthrough per feature, a "do them all" run, and what the person has finished.
// Pure data and helpers (no React): the screen is components/help/Tutorial.tsx.
import type { Os } from "./platform";

export type LinkKind =
  | { kind: "setting"; tab: string; label: string }
  | { kind: "help"; id: string; label: string }
  /** Puts this text in the chat box (or sends nothing): it never switches a feature on. */
  | { kind: "try"; text: string; label: string };

/** A live yes/no shown on a step. The screen answers them from the app's own state. */
export type CheckId = "speechModel" | "heyByteOn" | "pcControlOn" | "screenAskOn" | "replayOn" | "browserOn" | "routinesOn" | "findFilesOn" | "cloudConnected";

export interface Step {
  id: string;
  title: string;
  /** Short paragraphs; a line starting with "- " is a bullet. */
  body: string[];
  /** Something to say or type. */
  example?: string;
  links?: LinkKind[];
  check?: { id: CheckId; label: string };
  /** What to do when this step does not work. */
  ifStuck?: string;
  /** Only on this system (default: both). */
  os?: "windows" | "mac";
}

export interface Tutorial {
  id: string;
  title: string;
  /** One line: what you will be able to do. */
  promise: string;
  minutes: number;
  /** Icon name the screen maps to a picture. */
  icon: string;
  /** Listens, looks at the screen or controls the PC, and is off until switched on. */
  offByDefault?: boolean;
  /** Windows only (default: both). */
  os?: "windows";
  steps: Step[];
}

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });
const T = (text: string): LinkKind => ({ kind: "try", text, label: "Try it now: put this in the chat box" });

export const TUTORIALS: Tutorial[] = [
  {
    id: "getting-started",
    title: "Getting started",
    promise: "Pick a model and ask your first question",
    minutes: 3,
    icon: "rocket",
    steps: [
      { id: "what", title: "What BYTE is", body: ["BYTE is an assistant that lives on your computer. Your chats, files and notes stay here. Only web searches and downloads use the internet.", "You can type or talk to it, and it can search, write, read your files and, if you allow it, help with your computer."] },
      { id: "model", title: "Choose a model", body: ["A model is the brain BYTE thinks with. Bigger ones write better but need more memory.", "- BYTE looks at your computer and recommends one that fits.", "- You download it once. After that it works without the internet."], links: [S("models", "Open Models and speed")], ifStuck: "If the download stops, press Resume. It carries on where it left off." },
      { id: "ask", title: "Ask something", body: ["Type in the box at the bottom and press Enter. Shift+Enter starts a new line.", "Under the box, pick a mode: Fast for quick replies, Auto to let BYTE decide, Deep and Extended for careful, researched answers."], example: "Explain how a rainbow forms in three sentences.", links: [T("Explain how a rainbow forms in three sentences.")] },
      { id: "palette", title: "Find anything fast", body: ["Press the search shortcut to find any chat, setting or theme."], os: "windows", example: "Ctrl+K" },
      { id: "palette-mac", title: "Find anything fast", body: ["Press the search shortcut to find any chat, setting or theme."], os: "mac", example: "Command+K" },
    ],
  },
  {
    id: "chatting",
    title: "Chats, projects and private chats",
    promise: "Keep your chats tidy and private when you need to",
    minutes: 3,
    icon: "chat",
    steps: [
      { id: "history", title: "Your chats are saved", body: ["Every chat is saved on your computer, encrypted. Find old ones in the list on the left, or search them.", "You can pin, rename, move to a folder or delete a chat."] },
      { id: "private", title: "Private chats", body: ["A private chat is not saved and does not use or add memories. It never goes to the cloud, and the screen, browser and control features stay out of it.", "Use one for anything you don't want kept."] },
      { id: "memory", title: "What BYTE remembers", body: ["BYTE asks before it remembers something about you. You can see and delete every memory.", "- Turn memory on or off in Memory and notes."], links: [S("memory", "Open Memory and notes")] },
    ],
  },
  {
    id: "files",
    title: "Files and photos",
    promise: "Ask questions about your documents and pictures",
    minutes: 3,
    icon: "paperclip",
    steps: [
      { id: "attach", title: "Attach a file", body: ["Use the paperclip, or drop a file on the window. PDFs, Word, PowerPoint, Excel, text, code, photos and recordings all work.", "Scanned pages and photos of text are read on your computer."], example: "Summarize this document in five bullet points." },
      { id: "kb", title: "Your files as a knowledge base", body: ["Add folders and BYTE can answer \"what do my files say about...\" and name the file and page.", "- Pick the folders in Memory and notes.", "- Search by meaning needs one small download, offered there."], links: [S("memory", "Open Memory and notes")] },
      { id: "docs", title: "Make a document", body: ["Ask for a report, slide deck or Word document. BYTE writes an outline first so you can change it, then writes each section."], example: "Make a 10-slide deck about renewable energy." },
    ],
  },
  {
    id: "web",
    title: "Web search and the browser",
    promise: "Get answers with sources, and watch BYTE research",
    minutes: 4,
    icon: "globe",
    steps: [
      { id: "web", title: "Web search", body: ["Under the message box, Web can be Off, Auto (searches when a question needs fresh facts) or Always.", "Answers cite sources like [1]. Click one to open it."], example: "What are the latest results in the Premier League?", links: [T("What are the latest results in the Premier League?")] },
      { id: "deep", title: "Deep research", body: ["Deep and Extended modes read many sources and write a fuller answer. They take longer."] },
      { id: "cite", title: "Cite your sources", body: ["Press Cite on an answer to copy a reference in APA, MLA and other styles. You can also ask BYTE to fact-check a claim or compare two products."], example: "Fact-check: the Great Wall of China is visible from space." },
      { id: "browser", title: "The browser (off by default)", os: "windows", body: ["BYTE can research in a window you can watch, or drive its own Edge or Chrome window. It asks before sending or buying anything, never types passwords and blocks banking sites.", "To use it: turn on Watch BYTE browse, Live research or Use Edge or Chrome."], example: "Research the best budget monitors in the browser.", links: [S("control", "Open Control and screen"), H("browser", "Read about the browser")], check: { id: "browserOn", label: "Browser features are on" } },
    ],
  },
  {
    id: "voice",
    title: "Talking and listening",
    promise: "Dictate messages and hear answers read aloud",
    minutes: 3,
    icon: "mic",
    steps: [
      { id: "dictate", title: "Speak instead of typing", body: ["Click the microphone, or hold Space in an empty message box, and talk. What you say appears for you to check and send.", "This needs a speech model, a small download."], links: [S("voice", "Open Voice and Hey BYTE")], check: { id: "speechModel", label: "A speech model is installed" }, ifStuck: "If nothing is heard, check that your computer lets BYTE use the microphone." },
      { id: "readaloud", title: "Hear answers", body: ["Press the speaker under any answer to hear it. Turn on Read answers aloud to hear every answer.", "You can pick BYTE's voice from over two thousand free ones."], links: [S("voice", "Open Voice and Hey BYTE")], ifStuck: "No sound? Check the volume and that the right speakers are chosen in your computer's sound settings." },
      { id: "talk", title: "Talk mode", body: ["The sound-wave button starts hands-free talking: you speak, pause, hear the answer, and speak again. Say \"stop\" to end."] },
    ],
  },
  {
    id: "hey-byte",
    title: "Hey BYTE",
    promise: "Talk to BYTE hands-free from anywhere",
    minutes: 4,
    icon: "wave",
    offByDefault: true,
    steps: [
      { id: "what", title: "What it does", body: ["Say \"Hey BYTE\" and a question and BYTE answers out loud, like a speaker.", "It is off until you turn it on. While it is on, BYTE listens through your microphone for the phrase. Nothing is kept or sent until it hears it."] },
      { id: "setup", title: "Turn it on", body: ["- Install the speech model if it isn't installed yet.", "- Turn on Listen for Hey BYTE, and allow the microphone if your computer asks."], links: [S("voice", "Open Voice and Hey BYTE")], check: { id: "speechModel", label: "A speech model is installed" }, ifStuck: "If the switch is grey, the speech model is missing. If BYTE hears nothing, check microphone access in your computer's privacy settings." },
      { id: "status", title: "Check it is on", body: ["This shows whether the switch is on right now."], check: { id: "heyByteOn", label: "Listening for Hey BYTE is on" } },
      { id: "use", title: "Say it", body: ["You can say it in one breath, or say Hey BYTE, wait for the chime, then ask.", "After an answer BYTE keeps listening for a few seconds, so you can ask a follow-up without the phrase. Say \"thanks\" to end it."], example: "Hey BYTE, set a timer for ten minutes." },
      { id: "more", title: "Make it yours", body: ["- Show a window when you say it, if you want one.", "- Choose the chime, and a listening pill.", "- Wait until I'm done joins your sentence if you pause on a word like \"and\".", "- A talk key lets you talk without the phrase."], links: [S("voice", "Open Voice and Hey BYTE")] },
    ],
  },
  {
    id: "barge-in",
    title: "Interrupting and focus",
    promise: "Talk over BYTE and keep other voices out",
    minutes: 3,
    icon: "hand",
    os: "windows",
    offByDefault: true,
    steps: [
      { id: "bargein", title: "Talk over BYTE", body: ["Both options are off until you choose them.", "- Headphones: BYTE stops as soon as you speak. Use this with headphones.", "- Speakers: BYTE removes its own voice from the microphone first. It needs tuning for your room, so treat it as experimental."], links: [S("voice", "Open Voice and Hey BYTE")], example: "Ask for a long story, then start talking.", ifStuck: "If BYTE cuts itself off on speakers, switch to Off or Headphones." },
      { id: "focus", title: "Focus on one voice", body: ["Focus on one voice makes BYTE answer only the voice that said Hey BYTE, so a phone call or the TV doesn't interrupt. Say \"listen to everyone\" or \"stop focusing on me\" to change it.", "It is not tuned on many real voices yet, so expect mistakes."], links: [S("voice", "Open Voice and Hey BYTE")] },
      { id: "calls", title: "Calls and games", body: ["Hey BYTE stays off while another program is using the microphone for a call, and plays no chime during a full-screen game. You can turn these off in the same place."], links: [S("voice", "Open Voice and Hey BYTE")] },
    ],
  },
  {
    id: "quick-ask",
    title: "Quick Ask",
    promise: "Ask BYTE from any program",
    minutes: 2,
    icon: "bolt",
    steps: [
      { id: "windows", title: "Open it", os: "windows", body: ["Press Ctrl+Alt+Space in any program for a small ask box. Type or speak, and press Enter."], example: "Ctrl+Alt+Space" },
      { id: "mac", title: "Open it", os: "mac", body: ["Press Option+Space in any program for a small ask box. Type or speak, and press Enter."], example: "Option+Space" },
      { id: "speak", title: "Speak instead of typing", body: ["Quick Ask has a microphone. Click it and talk. You need a speech model for this; see Talking and listening."], links: [S("voice", "Open Voice and Hey BYTE")] },
      { id: "selected", title: "Work on selected text", body: ["Select text in any program and press the selected-text shortcut. BYTE can reply, explain or rewrite it. You can change both shortcuts in General."], links: [S("general", "Open General")] },
    ],
  },
  {
    id: "helpers",
    title: "Timers, notes and tasks",
    promise: "Set timers, save notes and keep a to-do list by talking",
    minutes: 3,
    icon: "clock",
    steps: [
      { id: "timers", title: "Timers and reminders", body: ["Say or type what you need. Timers ring with a sound and a notice."], example: "Set a timer for ten minutes.", links: [T("Set a timer for ten minutes.")] },
      { id: "notes", title: "Notes", body: ["Start with \"note:\" to save a quick note, or \"todo:\" to add a task.", "Ask \"where did I leave off?\" for a recap of your latest chats, notes and tasks."], example: "note: call the dentist" },
      { id: "brief", title: "Daily briefing", body: ["Say \"give me my briefing\" for today's weather, tasks and news you follow. You can also have it every morning, and spoken out loud, in Voice and Hey BYTE."], links: [S("voice", "Open Voice and Hey BYTE")] },
      { id: "routines", title: "Routines (off by default)", os: "windows", body: ["A routine runs several steps with one phrase, like \"game time\". Turn on Voice routines, then add your own."], links: [S("control", "Open Control and screen")], check: { id: "routinesOn", label: "Routines are on" } },
    ],
  },
  {
    id: "pc-control",
    title: "PC control",
    promise: "Let BYTE use your PC for you",
    minutes: 5,
    icon: "pointer",
    os: "windows",
    offByDefault: true,
    steps: [
      { id: "what", title: "What it can do", body: ["When you turn it on, BYTE reads the window in front as a list of buttons and boxes and clicks, types and presses keys for you.", "It shows an approval card before anything risky, and a bar with a Stop button while it works."] },
      { id: "on", title: "Turn it on", body: ["It stays off until you turn on Let BYTE use this PC for you. You can also choose how long a run may last and how many steps it may take."], links: [S("control", "Open Control and screen"), H("pc-control", "Read about PC control")], check: { id: "pcControlOn", label: "PC control is on" } },
      { id: "try", title: "Try it safely", body: ["Start with something harmless. Watch the bar at the top, and press Stop any time."], example: "Open Notepad.", links: [T("Open Notepad.")] },
      { id: "mode", title: "Commands or conversation", body: ["How BYTE handles commands decides when it acts. Smart acts on clear commands and talks about the rest. Always acts on anything that matches. Chat only acts when your message starts with \"do:\"."], links: [S("control", "Open Control and screen"), H("commands-or-conversation", "Read about this setting")], example: "do: set the volume to 30" },
      { id: "safe", title: "What stays safe", body: ["- Ask before risky steps is on by default.", "- Never ask, just do it is optional, and program files and dangerous key presses are always refused.", "- What BYTE did lists every action. Typed text is never stored."], ifStuck: "If BYTE does the wrong thing, press Stop on the bar." },
    ],
  },
  {
    id: "find-files",
    title: "Find files by voice",
    promise: "Say what you're looking for and see it in File Explorer",
    minutes: 2,
    icon: "folder",
    os: "windows",
    offByDefault: true,
    steps: [
      { id: "on", title: "Choose where it may look", body: ["Turn on Find files by voice and pick the folders BYTE may search. It searches only those."], links: [S("control", "Open Control and screen")], check: { id: "findFilesOn", label: "Find files is on" } },
      { id: "try", title: "Ask for a file", body: ["BYTE opens File Explorer with the file selected."], example: "Find my tax return.", ifStuck: "If nothing is found, check the folder is in your list." },
      { id: "status", title: "How's my PC?", body: ["Ask for a short spoken status of memory, disk, processor and graphics card."], example: "How's my PC?", links: [T("How's my PC?")] },
    ],
  },
  {
    id: "screen",
    title: "Screen questions and replay",
    promise: "Ask about what is on your screen",
    minutes: 4,
    icon: "screen",
    os: "windows",
    offByDefault: true,
    steps: [
      { id: "privacy", title: "Privacy first", body: ["Everything here is off until you turn it on. BYTE never looks unless you ask. The replay has its own switch and a badge while it records.", "Private chats never capture anything."] },
      { id: "ask", title: "Ask about the screen", body: ["Turn on Ask about what's on my screen. BYTE then looks at the window in front once, reads its text and answers."], example: "Explain this error.", links: [S("control", "Open Control and screen")], check: { id: "screenAskOn", label: "Screen questions are on" } },
      { id: "replay", title: "The 30-second replay", body: ["If you turn it on, BYTE keeps the last 30 seconds of your screen in memory only. It is never saved or sent, and is gone when you quit, switch it off or open a private chat.", "It stops when a password manager, banking page or private browser window is in front. You can edit that list."], links: [S("control", "Open Control and screen"), H("screen", "Read about the replay")], check: { id: "replayOn", label: "The replay is on" } },
      { id: "review", title: "Where did I go wrong?", body: ["After a round, ask. BYTE looks at a few still pictures, so it can describe what happened and a likely mistake, but it cannot judge aim or reaction time."], example: "Where did I go wrong?" },
      { id: "read", title: "Read it to me", body: ["BYTE can read your selected text or the window's text aloud. Turn on Read it to me."], links: [S("control", "Open Control and screen")] },
    ],
  },
  {
    id: "guided",
    title: "Guided help",
    promise: "Be shown how to do something, or have BYTE do it",
    minutes: 3,
    icon: "compass",
    os: "windows",
    offByDefault: true,
    steps: [
      { id: "what", title: "Two ways to help", body: ["Show me as I go: BYTE points at each step and waits while you do it.", "Do it for me: BYTE does the steps itself, with PC control's safety rules.", "Turn on Guided help to use either. It is off until you do, and the screen is only used while a guide is running."], links: [S("control", "Open Control and screen"), H("guided-help", "Read about guided help")] },
      { id: "ask", title: "How you ask", body: ["Ask how to do something in the program in front of you and BYTE offers a guide. While it shows you, F8 goes to the next step, F9 looks again and Esc stops."], example: "How do I turn on dark mode?" },
      { id: "stop", title: "Stay in charge", body: ["You can stop a guide at any time. BYTE clicks nothing in Show me as I go, and Do it for me shows the same Stop bar as PC control."], links: [S("control", "Open Control and screen")], ifStuck: "If the guide points at the wrong place, stop it and ask again with the program in front." },
    ],
  },
  {
    id: "gaming",
    title: "Gaming and where the model lives",
    promise: "Keep games fast while BYTE is on",
    minutes: 3,
    icon: "game",
    os: "windows",
    steps: [
      { id: "where", title: "Where the model lives", body: ["Models can run on the graphics card, the graphics card plus memory, or the processor only. Processor only keeps your graphics card free for games."], links: [S("models", "Open Models and speed"), H("gaming-and-model-memory", "Read about gaming")] },
      { id: "auto", title: "Game-aware answers", body: ["While a full-screen game runs, spoken questions can be answered from BYTE Cloud so the game doesn't stutter. This needs a BYTE Cloud account."], links: [S("cloud", "Open Account and cloud")], check: { id: "cloudConnected", label: "Connected to BYTE Cloud" } },
      { id: "quiet", title: "Quiet while you play", body: ["No chime after Hey BYTE during a game, and spoken notices wait. Timers still ring."], links: [S("voice", "Open Voice and Hey BYTE")] },
    ],
  },
  {
    id: "backup",
    title: "Backup and problem reports",
    promise: "Save your chats and settings, and report problems safely",
    minutes: 3,
    icon: "save",
    steps: [
      { id: "save", title: "Save a copy", body: ["Save a copy writes your settings and chats to a file you choose, locked with a passphrase. Your cloud key and account details are not in it.", "- Keep the passphrase: without it the file cannot be opened."], links: [S("backup", "Open Backup and help")] },
      { id: "restore", title: "Restore", body: ["Restore from a file asks for the passphrase, then BYTE restarts. A copy of what was replaced is kept."], links: [S("backup", "Open Backup and help")] },
      { id: "report", title: "Problem report", body: ["Prepare a report to see exactly what would be saved: version, system and hardware names and the end of BYTE's log, with your name and keys taken out. You can edit it. BYTE sends nothing."], links: [S("backup", "Open Backup and help")] },
      { id: "disk", title: "Free up space", body: ["Free up space lists BYTE's own big files. Nothing is deleted unless you press a button and confirm."], links: [S("models", "Open Models and speed")] },
    ],
  },
  {
    id: "privacy",
    title: "Privacy at a glance",
    promise: "Know what stays on your computer",
    minutes: 3,
    icon: "shield",
    steps: [
      { id: "local", title: "What stays here", body: ["Chats, memories, files, notes and settings are stored encrypted on your computer. The model runs here.", "Only web searches, downloads and BYTE Cloud use the internet. Work offline turns all of that off."], links: [S("privacy", "Open Privacy")] },
      { id: "off", title: "What is off until you turn it on", body: ["Hey BYTE, PC control, routines, finding files, the browser, screen questions, the replay, and spoken notices. Each has its own switch."], os: "windows", links: [S("privacy", "Open Privacy")] },
      { id: "offmac", title: "What is off until you turn it on", body: ["Hey BYTE and Mac control are off until you turn them on, each with its own switch."], os: "mac", links: [S("privacy", "Open Privacy")] },
      { id: "private", title: "Private chats and kids mode", body: ["Private chats are never saved and never use memories. Kids mode, with a PIN, limits BYTE to child-friendly answers and hides the rest."], links: [H("privacy", "Read the privacy article")] },
    ],
  },
  {
    id: "troubleshooting",
    title: "Troubleshooting and help",
    promise: "Fix the common problems",
    minutes: 3,
    icon: "lifebuoy",
    steps: [
      { id: "mic", title: "BYTE can't hear me", body: ["- Check that your computer lets BYTE use the microphone.", "- Check the right microphone is chosen and not muted.", "- Check the speech model is installed."], links: [S("voice", "Open Voice and Hey BYTE"), H("troubleshooting", "Read troubleshooting")] },
      { id: "sound", title: "No sound", body: ["- Check the volume and the chosen speakers.", "- After plugging in a headset, BYTE follows the new device. If it stays silent, set the output again and try once more."], links: [H("troubleshooting", "Read troubleshooting")] },
      { id: "open", title: "A warning when opening BYTE", body: ["Windows may say it protected your PC the first time. Choose More info, then Run anyway. BYTE isn't signed with a paid certificate."], os: "windows" },
      { id: "openmac", title: "A warning when opening BYTE", body: ["macOS may say BYTE can't be opened. Right-click it, choose Open, then Open Anyway."], os: "mac" },
      { id: "more", title: "More help", body: ["The Help window has an article for every feature, and it works offline. You can also just ask BYTE how to do something."], links: [H("getting-started", "Open the help articles")] },
    ],
  },
];

// ---------------------------------------------------------------- filtering

export function osKey(os: Os): "windows" | "mac" {
  return os === "mac" ? "mac" : "windows";
}

/** The steps of a tutorial on this system. */
export function stepsFor(t: Tutorial, os: Os): Step[] {
  const key = osKey(os);
  return t.steps.filter((s) => !s.os || s.os === key);
}

/** The tutorials that exist on this system, in the stable order. */
export function tutorialsFor(os: Os, all: Tutorial[] = TUTORIALS): Tutorial[] {
  const key = osKey(os);
  return all.filter((t) => !t.os || t.os === key);
}

// ---------------------------------------------------------------- progress

export interface Progress {
  /** Finished tutorial ids. */
  done: string[];
  /** The step each unfinished tutorial was left on. */
  step: Record<string, number>;
  /** "Do the whole tutorial" is running, and which ones were skipped in it. */
  all: { active: boolean; skipped: string[] };
}

export const EMPTY: Progress = { done: [], step: {}, all: { active: false, skipped: [] } };
export const KEY = "byte.tutorial.v1";

export interface Store {
  getItem(k: string): string | null;
  setItem(k: string, v: string): void;
}

const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/** Reads progress; broken or missing storage gives an empty record, never an error. */
export function loadProgress(store: Store | null | undefined): Progress {
  try {
    const raw = store?.getItem(KEY);
    if (!raw) return structuredClone(EMPTY);
    const v = JSON.parse(raw) as Partial<Progress>;
    const step: Record<string, number> = {};
    for (const [k, n] of Object.entries(v.step ?? {})) if (typeof n === "number" && Number.isFinite(n) && n >= 0) step[k] = Math.floor(n);
    return { done: strings(v.done), step, all: { active: !!v.all?.active, skipped: strings(v.all?.skipped) } };
  } catch {
    return structuredClone(EMPTY);
  }
}

export function saveProgress(store: Store | null | undefined, p: Progress): void {
  try {
    store?.setItem(KEY, JSON.stringify(p));
  } catch {
    /* storage may be blocked: the tutorial still works, it just forgets */
  }
}

export type Status = "done" | "progress" | "new";

export function statusOf(p: Progress, id: string): Status {
  if (p.done.includes(id)) return "done";
  return (p.step[id] ?? 0) > 0 ? "progress" : "new";
}

export function withStep(p: Progress, id: string, step: number): Progress {
  return { ...p, step: { ...p.step, [id]: Math.max(0, step) } };
}

export function withDone(p: Progress, id: string): Progress {
  const step = { ...p.step };
  delete step[id];
  return { ...p, done: p.done.includes(id) ? p.done : [...p.done, id], step };
}

/** The tutorial to suggest next: one already started, else the first not done. None when everything is done. */
export function recommended(list: Tutorial[], p: Progress): Tutorial | null {
  return list.find((t) => statusOf(p, t.id) === "progress") ?? list.find((t) => statusOf(p, t.id) === "new") ?? null;
}

// ---------------------------------------------------------------- do them all

/** The order "Do the whole tutorial" visits: every tutorial once, in order, without the skipped ones. */
export function doAllQueue(list: Tutorial[], skipped: string[]): string[] {
  return list.filter((t) => !skipped.includes(t.id)).map((t) => t.id);
}

/** The tutorial after `id` in the run, or null at the end. */
export function nextInRun(list: Tutorial[], skipped: string[], id: string): string | null {
  const q = doAllQueue(list, skipped);
  const i = q.indexOf(id);
  return i >= 0 && i + 1 < q.length ? q[i + 1] : null;
}

/** Where a run starts or resumes: the first unfinished, unskipped tutorial. */
export function resumePoint(list: Tutorial[], p: Progress): string | null {
  return doAllQueue(list, p.all.skipped).find((id) => !p.done.includes(id)) ?? null;
}

/** "Tutorial 4 of 17" in a run (counting only the ones not skipped). */
export function runPosition(list: Tutorial[], skipped: string[], id: string): { n: number; of: number } {
  const q = doAllQueue(list, skipped);
  return { n: Math.max(0, q.indexOf(id)) + 1, of: q.length };
}

/** The tutorial to offer after finishing `id` outside a run: the next one in order that is not done. */
export function relatedNext(list: Tutorial[], p: Progress, id: string): Tutorial | null {
  const i = list.findIndex((t) => t.id === id);
  const after = [...list.slice(i + 1), ...list.slice(0, Math.max(0, i))];
  return after.find((t) => !p.done.includes(t.id)) ?? null;
}

// ---------------------------------------------------------------- search

export function searchTutorials(list: Tutorial[], query: string): Tutorial[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return list;
  return list.filter((t) => {
    const text = `${t.title} ${t.promise} ${t.steps.map((s) => `${s.title} ${s.body.join(" ")} ${s.example ?? ""}`).join(" ")}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
}

/** Every setting group and help article a tutorial points to (for the tests). */
export function linksOf(t: Tutorial): LinkKind[] {
  return t.steps.flatMap((s) => s.links ?? []);
}
