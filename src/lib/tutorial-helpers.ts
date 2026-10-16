// Tutorial lessons for the everyday helpers (own file; registered with one line in tutorial.ts).
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const HELPER_TUTORIALS: Tutorial[] = [
  {
    id: "pc-helpers",
    title: "Everyday helpers",
    promise: "Get quick help in any app: read a box on screen, explain selected text, understand an error window, act on a copied number",
    minutes: 4,
    icon: "bolt",
    offByDefault: true,
    steps: [
      {
        id: "where",
        title: "Choose where questions are answered",
        body: ["Every helper asks a model. **Local** answers on this computer and is the default. **Auto** uses this computer and only falls back to BYTE Cloud for text that did not come from your screen. **Cloud** is the only choice that lets text from your screen leave this computer.", "Each helper is off until you switch it on, and each one is only for you on this computer."],
        links: [S("control", "Open Control and screen")],
      },
      {
        id: "region",
        title: "Read a box on the screen",
        os: "windows",
        body: ["Turn on **Region screenshot**, press its shortcut, and drag a box. BYTE reads only that box and forgets the picture at once. Then choose: what's this, translate, copy the text, make a note, or ask a question.", "A password manager, a bank or a private browser window under the box means nothing is read."],
        links: [S("control", "Open Control and screen"), H("pc-helpers", "Read about the helpers")],
        ifStuck: "If the box comes back with no words, it is probably a picture. BYTE reads text, not pictures.",
      },
      {
        id: "explain",
        title: "Explain selected text",
        body: ["Turn on **Explain this**, select some text in any app, and press its shortcut. A small popup next to the pointer explains it. It never takes the keyboard from the app you are reading and no chat opens."],
        example: "Select a confusing sentence, then press the shortcut.",
        ifStuck: "In a terminal or code editor BYTE will not press keys for you. Copy the text yourself, or use the region helper.",
      },
      {
        id: "dialogs",
        title: "An error window you don't understand",
        os: "windows",
        body: ["Turn on **Error window helper**. When a new error or installer window appears, BYTE's tray icon gets a red dot. Click the icon and BYTE reads the words that window shows (not a picture) and says what happened and what to try."],
        links: [S("control", "Open Control and screen")],
      },
      {
        id: "paste",
        title: "A toast when you copy something useful",
        body: ["Turn on **Smart copy toast**. Copy a parcel tracking number, an address, a link or a one-time code and a small toast offers the useful thing: track it, show it on a map, open the link or copy it without tracking bits, or clear the code.", "It never appears for anything that looks like a password or a key, and nothing is looked up on its own."],
        example: "Copy: 1Z999AA10123456784",
      },
    ],
  },
  {
    id: "pc-helpers-more",
    title: "More everyday helpers",
    promise: "Look at a page and get help, write up a meeting, type by speaking",
    minutes: 4,
    icon: "bolt",
    offByDefault: true,
    os: "windows",
    steps: [
      {
        id: "where2",
        title: "Remember where questions go",
        body: ["These helpers use the same **Ask where** choice as the others: Local (this computer) is the default, and only Cloud lets text from your screen leave this computer."],
        links: [S("control", "Open Control and screen")],
      },
      {
        id: "page",
        title: "Look at this page and help",
        os: "windows",
        body: ["Turn on **Look at this page and help**. Right-click BYTE's tray icon, choose **Look at this page and help** and drag a box. BYTE reads only that box and recommends what to do. With guided help on it can show you where to click, and with PC control on it can do it for you.", "To learn more about a web page, select its address in the address bar and press the shortcut."],
        links: [S("control", "Open Control and screen")],
      },
      {
        id: "meeting",
        title: "Write up a meeting",
        os: "windows",
        body: ["Turn on **Meeting assistant**. Right-click BYTE's tray icon, choose **Record a meeting**, and tell the others you are recording. When you stop, BYTE saves a note with a summary, a to-do list and a transcript.", "It records only when you click, and the sound stays in memory."],
        ifStuck: "No words in the note usually means no speech model: Settings → Models → Voice.",
      },
      {
        id: "dictate",
        title: "Type by speaking",
        os: "windows",
        body: ["Turn on **Dictate anywhere**. Click in any text box, hold the shortcut, say what you want typed, and let go. It needs a speech model (Settings → Models → Voice).", "The microphone is open only while you hold the key. BYTE never types into a password box."],
        ifStuck: "If nothing is typed, the app may run as administrator (Windows blocks typing into it), or no speech model is installed.",
      },
    ],
  },
  {
    id: "pc-helpers-home",
    title: "Focus, tidy and home helpers",
    promise: "Focus without interruptions, tidy Downloads, share routines, control lights by voice",
    minutes: 4,
    icon: "bolt",
    offByDefault: true,
    os: "windows",
    steps: [
      {
        id: "focus",
        title: "A focus session",
        os: "windows",
        body: ["Turn on **Focus mode**. Right-click BYTE's tray icon, choose **Start a focus session** and say what you will work on. BYTE turns notifications off, times the session and nudges you to take breaks. They come back when it ends.", "Length and break reminders both have a No limit choice."],
        links: [S("control", "Open Control and screen")],
      },
      {
        id: "tidy",
        title: "Tidy your Downloads",
        body: ["Turn on **Downloads tidy-up**. When files pile up, BYTE offers to sort them into folders inside Downloads. It never deletes anything, and **Undo** puts everything back."],
        links: [H("focus-tidy-home", "Read about the helpers")],
      },
      {
        id: "recipes",
        title: "Share a routine",
        os: "windows",
        body: ["Turn on **Recipes**. Export a routine to a small file, or paste someone else's and read the review of every step before adding it. A recipe can never run a script."],
      },
      {
        id: "home",
        title: "Lights and plugs by voice",
        body: ["Turn on **Home Assistant**, type its address, paste an access token and press **Test**. Then say \"turn on the kitchen light\". BYTE only changes lights, plugs, fans and switches."],
        example: "Turn on the kitchen light.",
        ifStuck: "If BYTE says it can't reach Home Assistant, check the address works in a browser on this computer.",
      },
      {
        id: "game",
        title: "Review your last play",
        os: "windows",
        body: ["With **Game companion** and **Remember the last seconds** on, choose **Review my last play** in the tray menu after a round. BYTE looks back over the last seconds and says what may have gone wrong. It runs on this computer and works without internet."],
        links: [S("control", "Open Control and screen")],
      },
    ],
  },
  {
    id: "pc-helpers-voice",
    title: "Voice helpers",
    promise: "Move around by voice, have text read aloud with highlighting, caption what you hear, pick your own wake phrase",
    minutes: 4,
    icon: "mic",
    offByDefault: true,
    os: "windows",
    steps: [
      {
        id: "nav",
        title: "Move around by voice",
        body: ["Turn on **Voice navigation** (PC control must be on too). Say \"go back\", \"scroll to the bottom\", \"next tab\" or \"click Send\".", "Every step is logged, Stop works, and a risky button still asks first."],
        example: "Hey BYTE, go back.",
        links: [S("control", "Open Control and screen"), H("voice-helpers", "Read about voice helpers")],
      },
      {
        id: "read",
        title: "Read aloud with highlighting",
        body: ["Turn on **Read aloud**. Select some text, press the shortcut, and watch the sentence being spoken light up. Pick Slow, Normal or Fast. Press the shortcut again to stop."],
        ifStuck: "If nothing is said, download one of BYTE's voices first (Settings → Voice and Hey BYTE).",
      },
      {
        id: "captions",
        title: "Captions and what they just said",
        body: ["Turn on **Live captions**, then choose **Start live captions** in the tray menu. BYTE writes what this computer is playing, and **What did they just say?** turns the last 30 seconds into words. Only memory is used."],
        ifStuck: "Captions need a speech model (Settings → Models → Voice). A smaller model lags less.",
      },
      {
        id: "wake",
        title: "Your own wake phrase",
        body: ["Type a phrase of your own under **Your own wake phrase**, like \"hey jarvis\". Leave it empty for \"Hey BYTE\". A longer, unusual phrase works better than a short common word."],
        links: [S("voice", "Open Voice and Hey BYTE")],
      },
    ],
  },
];
