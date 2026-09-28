// @vitest-environment jsdom
import { describe, expect, it } from "vitest";

import { closeOpenFences, renderMarkdown } from "./markdown";

describe("renderMarkdown", () => {
  it("renders GitHub-flavoured markdown", () => {
    const html = renderMarkdown("# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done");
    expect(html).toContain("<h1");
    expect(html).toContain("<table>");
    expect(html).toContain("<li>");
  });

  it("strips scripts, handlers, iframes and inline styles from model output", () => {
    const html = renderMarkdown(
      '<script>alert(1)</script><img src=x onerror="alert(2)"><iframe src="https://evil"></iframe><p style="color:red">hi</p>[x](javascript:alert(3))',
    );
    expect(html).not.toMatch(/<script|onerror|<iframe|style=|href="javascript:/i);
    expect(html).toContain("hi");
  });

  it("never produces javascript: links", () => {
    const html = renderMarkdown("[click me](javascript:alert(1))\n\n<a href=\"javascript:alert(2)\">x</a>");
    expect(html).not.toMatch(/href="\s*javascript:/i);
  });

  it("marks links for external opening", () => {
    const html = renderMarkdown("[site](https://example.com)");
    expect(html).toContain('data-external="true"');
    expect(html).toContain('href="https://example.com"');
  });
});

describe("closeOpenFences", () => {
  it("closes an unterminated code fence while streaming", () => {
    expect(closeOpenFences("text\n```js\nconst a = 1")).toBe("text\n```js\nconst a = 1\n```");
    expect(closeOpenFences("```\na\n```")).toBe("```\na\n```");
  });
});

describe("citations", () => {
  const sources = [
    { n: 1, title: "Tauri 2.0", url: "https://v2.tauri.app/blog/tauri-20/" },
    { n: 2, title: 'A "quoted" <title>', url: "https://example.com/?a=1&b=2" },
  ];

  it("links known citation numbers", () => {
    const html = renderMarkdown("Tauri 2 shipped in 2024 [1][2].", sources);
    expect(html).toContain('<sup class="cite"><a');
    expect(html).toContain('href="https://v2.tauri.app/blog/tauri-20/"');
    expect(html.match(/class="cite"/g)?.length).toBe(2);
  });

  it("drops invented citations but leaves markdown links, indexes and code alone", () => {
    const html = renderMarkdown("See this [9]. Also [1](https://x.com), arr[1] and `a[2]`.\n\n```\nx = b[2]\n```", sources);
    expect(html).not.toContain('class="cite"');
    expect(html).not.toContain("[9]");
    expect(html).toContain("See this.");
    expect(html).toContain("arr[1]");
    expect(html).toContain("a[2]");
    expect(html).toContain("b[2]");
  });

  it("drops all citations when an answer has no sources", () => {
    expect(renderMarkdown("Rust 1.61 is latest [1].", [])).toContain("Rust 1.61 is latest.");
  });

  it("escapes titles safely", () => {
    const html = renderMarkdown("Fact [2].", sources);
    const doc = new DOMParser().parseFromString(html, "text/html");
    expect(doc.querySelector("title")).toBeNull();
    const a = doc.querySelector("sup.cite a")!;
    expect(a.getAttribute("title")).toBe('A "quoted" <title>');
    expect(a.getAttribute("href")).toBe("https://example.com/?a=1&b=2");
  });

  it("marks the research confidence line", () => {
    const html = renderMarkdown("Answer.\n\n**Confidence:** Likely — one good source.");
    expect(html).toContain('<p class="confidence confidence-likely"><strong>Confidence</strong> <span class="level">Likely</span> — one good source.</p>');
    expect(renderMarkdown("**Confidence:** unsure, thin sources")).toContain("confidence-unsure");
    expect(renderMarkdown("My confidence: verified")).not.toContain("class=\"confidence");
  });
});
