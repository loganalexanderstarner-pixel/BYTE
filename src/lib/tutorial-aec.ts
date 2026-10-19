// Tutorial lesson for talking over BYTE (own file; registered with one line in tutorial.ts).
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const AEC_TUTORIALS: Tutorial[] = [
  {
    id: "talking-over-byte",
    title: "Talking over BYTE",
    promise: "Interrupt BYTE while it speaks, or say Hey BYTE over it, even with ordinary speakers",
    minutes: 3,
    icon: "mic",
    offByDefault: true,
    steps: [
      {
        id: "mode",
        title: "Choose headphones or speakers",
        body: ["Turn on **Hey BYTE**, then choose how you listen under **Talking over BYTE**. **Headphones** means any speech stops BYTE. **Speakers** means BYTE first removes its own voice from the microphone, then listens for you.", "It is off until you choose one, and it never runs in private chats or kids mode."],
        links: [S("voice", "Open Voice and Hey BYTE")],
      },
      {
        id: "copy",
        title: "Use the sound card's copy (speakers, Windows)",
        os: "windows",
        body: ["With **Speakers**, turn on **Use the sound card's own copy of what the speakers play**. Windows then gives BYTE the exact sound it sends to the speakers, so BYTE can subtract it from the microphone precisely. Nothing is recorded or saved."],
        links: [S("voice", "Open Voice and Hey BYTE")],
        ifStuck: "If BYTE cuts itself off, turn the speaker volume down a little or switch to headphones.",
      },
      {
        id: "try",
        title: "Talk over it",
        body: ["Ask BYTE for something long, like a story. While it speaks, say something. BYTE turns down after a third of a second and stops after about four tenths, then listens to you.", "You can also say **Hey BYTE** over it, followed by your new question. BYTE stops and answers the new question without the wake phrase in it."],
        example: "Tell me a long story about a lighthouse.",
        ifStuck: "BYTE needs a second or two of its own voice to learn your room before it can be interrupted. Try again after the first sentence.",
      },
      {
        id: "log",
        title: "See why it did what it did",
        body: ["Turn on **Test log**. Lines starting with `[test] interrupt` say how loud your voice was against the leftover of BYTE's own, and why BYTE stopped or carried on. Turn the log off afterwards, because it records spoken words."],
        links: [S("voice", "Open Voice and Hey BYTE"), H("talking-over-byte", "Read about talking over BYTE")],
      },
    ],
  },
];
