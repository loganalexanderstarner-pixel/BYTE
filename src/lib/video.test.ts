import { describe, expect, it } from "vitest";

import { stamp, videoDocSpec, videoLink, videoText } from "./video";
import type { VideoCard } from "./types";

const v: VideoCard = {
  id: "dQw4w9WgXcQ",
  title: "Never Gonna Give You Up",
  channel: "Rick Astley",
  seconds: 213,
  thumbnail: "",
  language: "en",
  autoCaptions: false,
  tldr: "A pop song about commitment.",
  keyPoints: [{ start: 43, text: "The chorus" }],
  chapters: [
    { start: 0, title: "Intro", summary: "" },
    { start: 43, title: "Chorus", summary: "The famous chorus." },
  ],
};

describe("video summaries", () => {
  it("formats timestamps and links", () => {
    expect([stamp(5), stamp(754), stamp(3723)]).toEqual(["0:05", "12:34", "1:02:03"]);
    expect(videoLink("abc", 754)).toBe("https://youtu.be/abc?t=754");
    expect(videoLink("abc", 0)).toBe("https://youtu.be/abc");
  });

  it("copies and saves the summary", () => {
    const t = videoText(v);
    expect(t.split("\n")[0]).toBe("Never Gonna Give You Up (Rick Astley, 3:33)");
    expect(t).toContain("- [0:43] The chorus");
    expect(t).toContain("- [0:43] Chorus: The famous chorus.");
    const spec = videoDocSpec(v);
    expect(spec.sections.map((s) => s.title)).toEqual(["In short", "Key points", "0:00 · Intro", "0:43 · Chorus"]);
    expect(spec.sources[0].url).toBe("https://youtu.be/dQw4w9WgXcQ");
  });
});
