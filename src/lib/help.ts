// BYTE's offline help center: articles bundled from src/help/*.md, searched and linked here.
import { isWindows } from "./keys";
import { localize } from "./helpText";

const files = import.meta.glob("../help/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

export interface Article {
  /** "getting-started" (from "01-getting-started.md") */
  id: string;
  title: string;
  body: string;
}

/** The bundled articles, worded for a Mac or for Windows. */
export function articlesFor(windows: boolean, sources: Record<string, string> = files): Article[] {
  return Object.entries(sources)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path, raw]) => {
      const text = localize(raw, windows);
      const id = path.split("/").pop()!.replace(/^\d+-/, "").replace(/\.md$/, "");
      const title = /^#\s+(.+)$/m.exec(text)?.[1]?.trim() ?? id;
      return { id, title, body: text.replace(/^#\s+.+\n+/, "") };
    });
}

export const ARTICLES: Article[] = articlesFor(isWindows());

/** Articles matching every word, title hits first, then how often the words appear. */
export function searchHelp(query: string, articles: Article[] = ARTICLES): Article[] {
  const words = query.toLowerCase().split(/\s+/).filter((w) => w.length > 1);
  if (!words.length) return articles;
  return articles
    .map((a) => {
      const title = a.title.toLowerCase();
      const all = `${title} ${a.body.toLowerCase()}`;
      if (!words.every((w) => all.includes(w))) return null;
      const score = words.reduce((s, w) => s + (title.includes(w) ? 20 : 0) + all.split(w).length - 1, 0);
      return { a, score };
    })
    .filter((x): x is { a: Article; score: number } => !!x)
    .sort((x, y) => y.score - x.score)
    .map((x) => x.a);
}

/** Links inside articles: `help:<id>` opens another article, `byte-setting:<tab>` opens Settings there. */
export function linkTarget(href: string): { kind: "help"; id: string } | { kind: "setting"; tab: string } | null {
  if (href.startsWith("help:")) return { kind: "help", id: href.slice(5) };
  if (href.startsWith("byte-setting:")) return { kind: "setting", tab: href.slice(13) };
  return null;
}
