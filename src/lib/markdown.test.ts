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
