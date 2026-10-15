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
  {
    id: "updates-removal",
    title: "Updates, what's new and removing BYTE",
    promise: "Update safely, read what changed, and remove BYTE's data when you leave",
    minutes: 3,
    icon: "lifebuoy",
    steps: [
      { id: "update", title: "Installing an update", body: ["When a new version is out a banner appears. Install downloads it, checks BYTE's signature and restarts BYTE. Your chats and models are kept."], links: [S("general", "Open General")] },
      { id: "new", title: "What's new", body: ["The first time you open BYTE after an update, a card lists what changed. It works offline and shows once. Press Got it to close it."] },
      { id: "remove", title: "Remove BYTE's data", body: ["Before uninstalling, Backup and help can remove your chats and settings, your models, or both. It lists exactly what it will delete and asks you to type REMOVE.", "Your notes and backups are never touched."], links: [S("backup", "Open Backup and help")], ifStuck: "If a file is in use, close other programs that show BYTE's folder and try again." },
      { id: "uninstall", title: "Uninstalling", os: "windows", body: ["The Windows uninstaller has a Delete the application data checkbox. Leave it empty to keep your chats and models for a later install.", "It always removes the startup entry."], links: [H("updates-and-uninstalling", "Read about updates and uninstalling")] },
      { id: "uninstall-mac", title: "Uninstalling", os: "mac", body: ["Drag BYTE to the Bin. To remove your chats and models too, use Remove BYTE's data first."], links: [H("updates-and-uninstalling", "Read about updates and uninstalling")] },
    ],
  },
  {
    id: "laptop-mode",
    title: "Laptop mode",
    promise: "Save battery by pausing listening and the screen replay",
    minutes: 2,
    icon: "shield",
    offByDefault: true,
    steps: [
      { id: "what", title: "What it saves", body: ["On battery, BYTE pauses listening for Hey BYTE and the screen replay, and shows a small badge."], },
      { id: "on", title: "Turn it on", body: ["Laptop mode is off until you turn it on in General. Your saved settings are not changed; everything returns on mains power."], links: [S("general", "Open General")] },
      { id: "model", title: "A smaller model", body: ["The badge can switch to a smaller model you already downloaded, which uses less power. Switch back in Models and speed any time."], links: [S("models", "Open Models and speed")] },
    ],
  },
];
