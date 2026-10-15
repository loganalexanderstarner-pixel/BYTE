import { describe, expect, it } from "vitest";

import { cite, citeAll, plainCitation, splitName } from "./citations";
import type { Source } from "./types";

const read = new Date(2026, 8, 28);

const paper: Source = {
  n: 2,
  title: "Fast Inference from Transformers via Speculative Decoding",
  url: "https://arxiv.org/abs/2211.17192v2",
  snippet: "",
  read: true,
  meta: { authors: ["Yaniv Leviathan", "Matan Kalman", "Yossi Matias"], year: 2023, venue: "Proceedings of ICML", doi: "10.48550/arXiv.2211.17192" },
};

const web: Source = { n: 1, title: "Intermittent fasting", url: "https://en.wikipedia.org/wiki/Intermittent_fasting", snippet: "", read: true };

describe("citations", () => {
  it("splits names, keeping particles and initials", () => {
    expect(splitName("Ludwig van Beethoven")).toEqual({ first: ["Ludwig"], last: "van Beethoven" });
    expect(splitName("E. B. Hill")).toEqual({ first: ["E.", "B."], last: "Hill" });
    expect(splitName("Hill, Emily")).toEqual({ first: ["Emily"], last: "Hill" });
  });

  it("formats a paper in every style", () => {
    expect(cite(paper, "apa", read)).toBe(
      "Leviathan, Y., Kalman, M., & Matias, Y. (2023). Fast Inference from Transformers via Speculative Decoding. *Proceedings of ICML*. https://doi.org/10.48550/arXiv.2211.17192",
    );
    expect(cite(paper, "mla", read)).toBe(
      "Leviathan, Yaniv, et al. “Fast Inference from Transformers via Speculative Decoding.” *Proceedings of ICML*, 2023, doi.org/10.48550/arXiv.2211.17192.",
    );
    expect(cite(paper, "chicago", read)).toBe(
      "Leviathan, Yaniv, Matan Kalman, and Yossi Matias. 2023. “Fast Inference from Transformers via Speculative Decoding.” *Proceedings of ICML*. https://doi.org/10.48550/arXiv.2211.17192.",
    );
    expect(cite(paper, "harvard", read)).toBe(
      "Leviathan, Y., Kalman, M. and Matias, Y. (2023) ‘Fast Inference from Transformers via Speculative Decoding’, *Proceedings of ICML*. Available at: https://doi.org/10.48550/arXiv.2211.17192.",
    );
    expect(cite(paper, "ieee", read)).toBe(
      "[2] Y. Leviathan, M. Kalman, and Y. Matias, “Fast Inference from Transformers via Speculative Decoding,” *Proceedings of ICML*, 2023, doi: 10.48550/arXiv.2211.17192.",
    );
    const bib = cite(paper, "bibtex", read);
    expect(bib).toMatch(/^@article\{leviathan2023fast,/);
    expect(bib).toContain("author = {Leviathan, Yaniv and Kalman, Matan and Matias, Yossi}");
    expect(bib).toContain("doi = {10.48550/arXiv.2211.17192}");
  });

  it("formats a web page with the date it was read", () => {
    expect(cite(web, "apa", read)).toBe("Intermittent fasting. (n.d.). Wikipedia. Retrieved September 28, 2026, from https://en.wikipedia.org/wiki/Intermittent_fasting");
    expect(cite(web, "mla", read)).toBe("“Intermittent fasting.” *Wikipedia*, en.wikipedia.org/wiki/Intermittent_fasting. Accessed 28 Sept. 2026.");
    expect(cite(web, "ieee", read)).toContain("[1] “Intermittent fasting,” Wikipedia. [Online]. Available: https://en.wikipedia.org/wiki/Intermittent_fasting (accessed Sept. 28, 2026).");
    expect(cite(web, "bibtex", read)).toContain("note = {Accessed 2026-09-28}");
  });

  it("uses et al. for long author lists and handles no authors", () => {
    const many = { ...paper, meta: { ...paper.meta!, authors: ["A One", "B Two", "C Three", "D Four", "E Five", "F Six", "G Seven"] } };
    expect(cite(many, "harvard", read)).toMatch(/^One, A\. et al\. \(2023\)/);
    expect(cite(many, "ieee", read)).toMatch(/^\[2\] A\. One et al\., /);
    const none = { ...paper, meta: { ...paper.meta!, authors: [], year: null } };
    expect(cite(none, "apa", read)).toMatch(/^\(n\.d\.\)\. Fast Inference/);
  });

  it("copies all sources in order, without display markup", () => {
    const all = citeAll([paper, web], "apa", read);
    expect(all.split("\n")).toHaveLength(2);
    expect(all.startsWith("Intermittent fasting.")).toBe(true);
    expect(plainCitation("A. *Venue*. x")).toBe("A. Venue. x");
  });
});
