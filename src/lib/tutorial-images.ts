// Tutorial lesson for Image Studio (Windows; own file, registered with one line in tutorial.ts).
import type { LinkKind, Tutorial } from "./tutorial";

const S = (tab: string, label: string): LinkKind => ({ kind: "setting", tab, label });
const H = (id: string, label: string): LinkKind => ({ kind: "help", id, label });

export const IMAGE_TUTORIALS: Tutorial[] = [
  {
    id: "image-models",
    title: "Image models",
    promise: "See which picture-making models your PC can run, and pick a version that fits",
    minutes: 3,
    icon: "image",
    os: "windows",
    steps: [
      {
        id: "open",
        title: "Open the image catalog",
        body: ["In Models, choose **Image models** at the top. Every model is free, and everything runs on this PC. Making pictures arrives in the next test builds; for now you can look around."],
        links: [S("models", "Open Models"), H("image-models", "Read about image models")],
      },
      {
        id: "versions",
        title: "Pick a version that fits",
        body: [
          "Each model comes in several sizes, called versions. Smaller ones fit smaller graphics cards and lose a little quality.",
          "- The star marks the version BYTE recommends for your PC.",
          "- Under the versions, BYTE says if a version **fits video memory**, **overflows into RAM** (and by how much), or is **too big**. A version that overflows still works, just slower.",
        ],
      },
      {
        id: "filter",
        title: "Find what you want",
        body: ["Sort best to least, or by speed or size. Filter by style: Realism, Cartoon, Illustration, or **Poems and posters** for art with room for words. **Fits my graphics card** hides anything too big."],
        ifStuck: "If a filter hides everything, press All to clear it.",
      },
    ],
  },
];
