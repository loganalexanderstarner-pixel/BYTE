// BYTE's offline help center: articles bundled from src/help/*.md, searched and linked here.
import { currentOs, type Os } from "./platform";
import { localize } from "./helpText";

const files = import.meta.glob("../help/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

export interface Article {
  /** "getting-started" (from "01-getting-started.md") */
  id: string;
  title: string;
  body: string;
}

/** The bundled articles, worded for a Mac, Windows or Linux (a boolean is the old form: true is Windows). */
export function articlesFor(os: Os | boolean, sources: Record<string, string> = files): Article[] {
  return Object.entries(sources)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path, raw]) => {
      const text = localize(raw, os);
      const id = path.split("/").pop()!.replace(/^\d+-/, "").replace(/\.md$/, "");
      const title = /^#\s+(.+)$/m.exec(text)?.[1]?.trim() ?? id;
      return { id, title, body: text.replace(/^#\s+.+\n+/, "") };
    });
}

export const ARTICLES: Article[] = articlesFor(currentOs());

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

/** The help list is grouped by what someone needs; an article not named here lands in "More". */
export const HELP_SECTIONS: { title: string; ids: string[] }[] = [
  { title: "Start here", ids: ["getting-started", "guided-help"] },
  { title: "Chat, research and documents", ids: ["modes-and-web", "files", "documents", "notes", "everyday-helpers", "pc-helpers", "focus-tidy-home", "voice-helpers", "page-tools", "everyday"] },
  { title: "Talking to BYTE", ids: ["voice", "hey-byte", "interrupting-calls-games", "shortcuts"] },
  { title: "Controlling your PC", ids: ["mac-control", "pc-control", "browser", "screen", "commands-or-conversation"] },
  { title: "Models and speed", ids: ["models", "gaming-and-model-memory"] },
  { title: "Account, privacy and style", ids: ["cloud", "privacy", "personality"] },
  { title: "Fixing problems", ids: ["troubleshooting"] },
];

/** The articles in the order of HELP_SECTIONS, each section with its articles (empty sections left out). */
export function sectioned(articles: Article[]): { title: string; articles: Article[] }[] {
  const named = new Set(HELP_SECTIONS.flatMap((s) => s.ids));
  const out = HELP_SECTIONS.map((s) => ({ title: s.title, articles: s.ids.map((id) => articles.find((a) => a.id === id)).filter((a): a is Article => !!a) }));
  const rest = articles.filter((a) => !named.has(a.id));
  if (rest.length) out.push({ title: "More", articles: rest });
  return out.filter((s) => s.articles.length);
}
