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
      {
        id: "page",
        title: "Look at this page and help",
        os: "windows",
        body: ["Turn on **Look at this page and help**. Right-click BYTE's tray icon, choose **Look at this page and help** and drag a box. BYTE reads only that box and recommends what to do. With guided help on it can show you where to click, and with PC control on it can do it for you.", "To learn more about a web page, select its address in the address bar and press the shortcut."],
        links: [S("control", "Open Control and screen")],
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
];
