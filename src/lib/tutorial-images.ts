// Tutorial lesson for Image Studio (Windows; own file, registered with one line in tutorial.ts).
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const IMAGE_TUTORIALS: Tutorial[] = [
  {
    id: "image-models",
    title: "Making pictures",
    promise: "Make pictures from words, on your own PC",
    minutes: 5,
    icon: "image",
    os: "windows",
    steps: [
      {
        id: "turn-on",
        title: "Turn on picture making",
        body: ["Open **Images** in Settings and turn on **Make pictures**. It is off until you turn it on, and it stays off in kids mode. Everything runs on this PC; your words and photos never leave it."],
        links: [S("images", "Open Images"), H("image-models", "Read about making pictures")],
      },
      {
        id: "engine",
        title: "Download the picture engine",
        body: ["BYTE draws pictures with a small program it downloads once. It recommends the right one for your graphics card, and checks the file against a fixed checksum."],
        ifStuck: "If the download stops, press the button again. It carries on where it left off.",
      },
      {
        id: "model",
        title: "Pick a model and a version",
        body: [
          "Each model comes in several sizes, called versions. Smaller ones fit smaller graphics cards and lose a little quality.",
          "- The star marks the version BYTE recommends for your PC.",
          "- Under the versions, BYTE says if a version **fits video memory**, **overflows into RAM** (and by how much), or is **too big**. A version that overflows still works, just slower.",
          "- A good first model is **Z-Image Turbo**.",
        ],
      },
      {
        id: "make",
        title: "Make a picture",
        body: ["Describe it in **Make a picture** and press **Make picture**. A progress bar shows each step. Press **Stop** any time."],
        example: "A lighthouse on a rocky coast at golden hour, 35mm film photograph, soft haze over the water",
      },
      {
        id: "better",
        title: "Make it better",
        body: [
          "These switches are off until you turn them on:",
          "- **Detail pass** adds fine detail, and takes about twice as long.",
          "- **Enlarge each picture** makes it bigger. **4xNomosWebPhoto** keeps real skin texture.",
          "- **Camera finish** adds film grain and colour like a real camera, so pictures look less computer-made.",
        ],
      },
      {
        id: "extras",
        title: "The extras (each has its own switch)",
        body: [
          "All off until you turn them on:",
          "- **Prompt helper** rewrites a short idea into a fuller description.",
          "- **Compare models** makes the same picture with several models, side by side.",
          "- **Gallery** keeps what you made, with what you asked for.",
          "- **Edit photos** changes a photo with a sentence (remove a person, restore an old photo), or makes it bigger, crops it, adds grain or motion blur.",
          "- **Poem and quote posters** put words on a picture where it is quiet.",
          "- **Draw from chat and Hey BYTE** makes a picture when you say “draw me …”.",
        ],
        links: [H("image-tools", "Read about the extras")],
      },
      {
        id: "filter",
        title: "Find other models",
        body: ["Sort best to least, or by speed or size. Pick what you want to do, then a style. **Realism** has real-life looks such as portraits, food or landscapes. Models list what they are good at, and nothing is hidden by a filter you did not choose."],
        ifStuck: "If a filter hides everything, press All to clear it.",
      },
    ],
  },
];
