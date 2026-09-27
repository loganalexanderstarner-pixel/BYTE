import DOMPurify from "dompurify";
import { Marked } from "marked";

const marked = new Marked({ gfm: true, breaks: false, async: false });

/**
 * Renders model output to safe HTML. Model text is untrusted: scripts, event
 * handlers, iframes and styles are stripped, and links are marked so the app
 * opens them in the user's browser instead of inside BYTE.
 */
export function renderMarkdown(src: string): string {
  const html = marked.parse(src) as string;
  return sanitize(html);
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
