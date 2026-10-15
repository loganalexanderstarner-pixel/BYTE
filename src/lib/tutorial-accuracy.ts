// Tutorial lessons for the planning settings (accuracy track). Registered in tutorial.ts with one line.
import type { Tutorial } from "./tutorial";

export const ACCURACY_TUTORIALS: Tutorial[] = [
  {
    id: "planning",
    title: "Planning: tests, notes and dry run",
    promise: "Pick the best model for PC control, and let BYTE learn each program",
    minutes: 4,
    icon: "pointer",
    os: "windows",
    offByDefault: true,
    steps: [
      {
        id: "model",
        title: "Pick and test a model",
        body: ["In Models and speed, choose the PC control filter. The recommended model is first. Press Test to run 40 standard tasks and see a score; nothing is clicked or typed.", "Use for PC control picks it, and downloads it first if needed."],
        links: [{ kind: "setting", tab: "models", label: "Open Models and speed" }, { kind: "help", id: "planning-accuracy", label: "Read about planning" }],
      },
      {
        id: "dry",
        title: "Try a dry run",
        body: ["Start a message with “dry run:” and BYTE lists what it would click and type, without doing it. This needs PC control turned on first; it is off until you turn it on."],
        example: "dry run: open the calculator",
        links: [{ kind: "setting", tab: "control", label: "Open Control and screen" }],
        check: { id: "pcControlOn", label: "PC control is on" },
      },
      {
        id: "notes",
        title: "Notes about programs",
        body: ["Turn on Notes about programs under Planning. After a job works, BYTE keeps the names of the buttons it used and uses them next time. Nothing you typed is kept, and it stays on this PC.", "You can add your own notes, and delete any of them."],
        links: [{ kind: "setting", tab: "control", label: "Open Planning" }],
        ifStuck: "If you do not see Planning, turn on PC control or Guided help first.",
      },
      {
        id: "cloud",
        title: "Plan with BYTE Cloud (optional)",
        body: ["Cloud planning is off until you turn it on, and asks you once. It sends your request and the names of the buttons in the window in front, nothing else. Ask where stays on Local unless you change it."],
        links: [{ kind: "setting", tab: "control", label: "Open Planning" }],
      },
    ],
  },
];
