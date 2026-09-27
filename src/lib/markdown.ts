import DOMPurify from "dompurify";
import { Marked } from "marked";

const marked = new Marked({ gfm: true, breaks: false, async: false });

/**
 * Renders model output to safe HTML. Model text is untrusted: scripts, event
 * handlers, iframes and styles are stripped, and links are marked so the app
 * opens them in the user's browser instead of inside BYTE.
 */
export function renderMarkdown(src: string, sources?: CiteSource[]): string {
  const html = marked.parse(sources ? linkCitations(src, sources) : src) as string;
  return sanitize(html);
}

export interface CiteSource {
  n: number;
  title: string;
  url: string;
}

const escapeAttr = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

/**
 * Turns citation markers like [2] or [1][3] into small numbered links to the
 * source. Markers with no matching source were invented by the model and are
 * removed. Markdown links ("[2](…)"), indexes like `arr[1]` and code are left alone.
 */
export function linkCitations(src: string, sources: CiteSource[]): string {
  const byN = new Map(sources.map((s) => [s.n, s]));
  return src
    .split(/(```[\s\S]*?(?:```|$)|`[^`\n]*`)/g)
    .map((part, i) => {
      if (i % 2 === 1) return part; // code
      return part.replace(/(\s*)\[(\d{1,3})\](?!\()/g, (whole, space: string, num: string, offset: number, str: string) => {
        // "arr[1]": a number glued to a word is an index, not a citation.
        if (!space && /\w/.test(str[offset - 1] ?? "")) return whole;
        const s = byN.get(Number(num));
        if (!s) return ""; // invented citation: drop it with its leading space
        return `${space}<sup class="cite"><a href="${escapeAttr(s.url)}" title="${escapeAttr(s.title)}">${s.n}</a></sup>`;
      });
    })
    .join("");
}

export function sanitize(html: string): string {
  const clean = DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["style", "form", "input", "button", "iframe", "object", "embed"],
    FORBID_ATTR: ["style"],
  });
  return clean.replace(/<a (?![^>]*data-external)/g, '<a data-external="true" rel="noreferrer" ');
}

/**
 * While a reply is streaming, an unfinished code fence would swallow the rest
 * of the message. Close it temporarily so the preview renders correctly.
 */
export function closeOpenFences(src: string): string {
  const fences = src.match(/^\s*```/gm)?.length ?? 0;
  return fences % 2 === 1 ? `${src}\n\`\`\`` : src;
}
