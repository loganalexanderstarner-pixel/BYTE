// Tutorial lesson for the quick "Hey BYTE" detector (own file; registered with one line in tutorial.ts).
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const WAKEMODEL_TUTORIALS: Tutorial[] = [
  {
    id: "wake-detector",
    title: "The Hey BYTE detector",
    promise: "Choose how BYTE listens for “Hey BYTE”, and check that it hears you",
    minutes: 3,
    icon: "mic",
    offByDefault: true,
    os: "windows",
    steps: [
      {
        id: "two-ways",
        title: "Two ways to listen",
        body: [
          "**Careful** is how “Hey BYTE” always worked: the speech model reads every burst of speech and looks for the phrase.",
          "**Quick** adds a very small detector that hears every moment of sound. It only decides which bursts the speech model should read, so the speech model rests while you talk about other things. The speech model still confirms every guess, so the detector alone can never wake BYTE.",
        ],
        links: [S("voice", "Open Voice and Hey BYTE"), H("wake-detector", "Read about the detector")],
      },
      {
        id: "turn-on",
        title: "Turn on Hey BYTE first",
        body: ["The detector only matters while **Listen for “Hey BYTE”** is on. That switch is off until you turn it on, and it stays off in kids mode and private chats."],
        links: [S("voice", "Open Voice and Hey BYTE")],
      },
      {
        id: "choose",
        title: "Choose Quick or Careful",
        body: [
          "In **Wake word detector** pick **Quick (new)** or **Careful (speech model only)**.",
          "The detector was trained on computer-made voices, so it can miss a real voice more often than the speech model does. If you ever have to say “Hey BYTE” twice, go back to Careful.",
        ],
        links: [S("voice", "Open Voice and Hey BYTE")],
        ifStuck: "A wake phrase of your own always uses Careful, so the choice has no effect while one is set.",
      },
      {
        id: "try",
        title: "Say it a few ways",
        body: ["Say “Hey BYTE” normally, then quickly, then from across the room, then while music plays. Notice whether BYTE answers the first time."],
        example: "Hey BYTE, what time is it?",
        ifStuck: "If it misses you now and then, turn on the Test log in Voice and Hey BYTE: each guess of the detector is written as a line starting with “detector”, with its score.",
      },
      {
        id: "talk-over",
        title: "Say it over BYTE's own voice",
        body: ["With Quick on and **Talking over BYTE** set to **Speakers**, you can say “Hey BYTE” while BYTE is talking. BYTE stops and listens. This depends on the echo canceller, which needs tuning in your room."],
        links: [H("interrupting-calls-games", "Read about talking over BYTE")],
        ifStuck: "If BYTE keeps talking over your “Hey BYTE”, use headphones, or turn the volume down a little.",
      },
    ],
  },
];
