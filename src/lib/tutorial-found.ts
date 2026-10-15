// Lessons for the foundations track: first-run setup, safe mode, crash recovery, updates, uninstall, battery mode,
// the never-touch list, the daily summary, the on-screen approval card and research that keeps its pages.
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const FOUND_TUTORIALS: Tutorial[] = [
  {
    id: "first-run",
    title: "First-run setup",
    promise: "Choose your models and which optional features to switch on",
    minutes: 3,
    icon: "rocket",
    offByDefault: true,
    steps: [
      { id: "models", title: "Pick the chat model", body: ["The first time BYTE opens it checks your computer and shows models that fit. The first card is BYTE's pick.", "- You can change the model any time in Models and speed."], links: [S("models", "Open Models and speed")] },
      { id: "control-model", title: "The model for PC control", os: "windows", body: ["On Windows the same screen offers a small helper model that plans PC control. Both sizes are shown and added together.", "Ticking it only downloads it. PC control stays off until you turn it on."], links: [H("first-run-setup", "Read about first-run setup")] },
      { id: "features", title: "Each feature is a question", os: "windows", body: ["After the download BYTE asks, once and in plain words, about Hey BYTE, the browser, PC control, screen questions and guided help.", "Nothing is on unless you tick it. You can leave every box empty."] },
      { id: "features-mac", title: "Each feature is a question", os: "mac", body: ["After the download BYTE asks, once and in plain words, about Hey BYTE and the browser.", "Nothing is on unless you tick it. You can leave every box empty."] },
      { id: "later", title: "Change your mind later", body: ["Every feature has its own switch. If you skipped one, turn it on later in Settings. Skipping never counts as a no."], links: [S("voice", "Open Voice and Hey BYTE"), S("control", "Open Control and screen")] },
    ],
  },
  {
    id: "safe-mode",
    title: "Safe mode and crash recovery",
    promise: "Start BYTE with every optional feature off, and send a report after a crash",
    minutes: 3,
    icon: "lifebuoy",
    steps: [
      { id: "what", title: "What safe mode does", body: ["Safe mode starts BYTE with everything that listens, looks at the screen or controls things switched off for that run.", "Your saved settings are not changed. A normal start brings them all back."] },
      { id: "start", title: "Start it", body: ["- Press Restart in safe mode in Backup and help.", "- Or start BYTE with the option --safe.", "A banner at the top tells you it is on, with a button to restart normally."], links: [S("backup", "Open Backup and help")] },
      { id: "use", title: "When it helps", body: ["If BYTE keeps closing, start it in safe mode. If it works, one of the optional features was the cause: turn them back on one at a time."], ifStuck: "Safe mode refuses to turn those features on. Restart normally first." },
      { id: "crash", title: "After BYTE closes unexpectedly", body: ["Next time you open BYTE it says it closed unexpectedly and offers to prepare a problem report.", "You read the whole report, can change it, and save it to a file. Nothing is sent anywhere."], links: [H("safe-mode-and-crashes", "Read about safe mode and crashes")] },
    ],
  },
];
