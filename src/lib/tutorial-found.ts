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
];
