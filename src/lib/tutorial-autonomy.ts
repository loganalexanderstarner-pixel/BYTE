// The autonomous mode lesson (Windows). Registered in tutorial.ts with one line.
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const autonomyTutorials: Tutorial[] = [
  {
    id: "autonomous-mode",
    title: "Autonomous mode",
    promise: "Give BYTE a list of jobs and get a report when it is done",
    minutes: 6,
    icon: "pointer",
    os: "windows",
    offByDefault: true,
    steps: [
      { id: "what", title: "What it is", body: ["You give BYTE a job in plain words. It splits it into stages, shows you the list, and does them one after another while you are away. Then it writes you a report.", "It is off until you turn it on and accept a notice."] },
      { id: "on", title: "Turn it on", body: ["Turn on PC control first. Then turn on Autonomous mode and read the notice. It is off until you accept it."], links: [S("control", "Open Control and screen"), H("autonomous-mode", "Read about autonomous mode")], check: { id: "pcControlOn", label: "PC control is on" } },
      { id: "contract", title: "Set the run contract", body: ["Before a job starts you choose the programs and folders it may use, the people it may message, and a time, step and message budget. Every budget has an Unlimited choice.", "- It never deletes anything unless you allow it, and it never spends money.", "- Anything outside the contract pauses the job and waits for your answer."], ifStuck: "If a job keeps pausing, add the program it asked about to the contract." },
      { id: "stop", title: "Stopping it", body: ["A small window shows what BYTE is doing now and what comes next, with a big Stop button. You can also press the stop key (Ctrl+Alt+F12 unless you changed it), or turn on Pause when I use the PC."], },
      { id: "report", title: "The report", body: ["When the job ends or you stop it, BYTE saves a written report as a chat: what it did in each stage, what it read, what it sent and what it skipped. If BYTE closes in the middle, the job waits under Jobs and you can resume it."], links: [H("approve-and-dont-ask-again", "Read about how much BYTE asks")] },
    ],
  },
];
