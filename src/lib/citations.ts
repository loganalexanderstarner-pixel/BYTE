// Citation styles for answer sources: APA 7, MLA 9, Chicago (author-date),
// Harvard, IEEE, and BibTeX. Papers use their authors/year/venue/DOI
// (Rust `SourceMeta`); web pages use title, site and URL with the date read.

import type { Source } from "./types";

export type CiteStyle = "apa" | "mla" | "chicago" | "harvard" | "ieee" | "bibtex";

export const CITE_STYLES: { id: CiteStyle; label: string }[] = [
  { id: "apa", label: "APA" },
  { id: "mla", label: "MLA" },
  { id: "chicago", label: "Chicago" },
  { id: "harvard", label: "Harvard" },
  { id: "ieee", label: "IEEE" },
  { id: "bibtex", label: "BibTeX" },
];

interface Name {
  first: string[];
  last: string;
}

/** "Ada King Lovelace" → first ["Ada", "King"], last "Lovelace"; "E. B. Hill" keeps initials. */
export function splitName(full: string): Name {
  const t = full.trim();
  if (t.includes(",")) {
    const [last, first] = t.split(",", 2);
    return { last: last.trim(), first: first.trim().split(/\s+/).filter(Boolean) };
  }
  const parts = t.split(/\s+/).filter(Boolean);
  // Family-name particles stay with the last name ("Ludwig van Beethoven").
  let i = parts.length - 1;
  while (i > 1 && /^(van|von|de|der|den|del|da|di|le|la|du)$/i.test(parts[i - 1])) i--;
  return { first: parts.slice(0, i), last: parts.slice(i).join(" ") || t };
}

const initials = (n: Name, spaced = true) =>
  n.first.map((f) => f.split("-").map((p) => `${p[0]?.toUpperCase() ?? ""}.`).join("-")).join(spaced ? " " : "");

const lastInitials = (n: Name) => (n.first.length ? `${n.last}, ${initials(n)}` : n.last);
const lastFirst = (n: Name) => (n.first.length ? `${n.last}, ${n.first.join(" ")}` : n.last);
const firstLast = (n: Name) => [...n.first, n.last].join(" ");
const initialsLast = (n: Name) => (n.first.length ? `${initials(n)} ${n.last}` : n.last);

function joinList(items: string[], and: string, oxford: boolean): string {
  if (items.length <= 1) return items.join("");
  if (items.length === 2) return `${items[0]} ${and} ${items[1]}`;
  return `${items.slice(0, -1).join(", ")}${oxford ? "," : ""} ${and} ${items[items.length - 1]}`;
}

function host(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

/** A site's display name: "en.wikipedia.org" → "Wikipedia", "nytimes.com" → "nytimes.com". */
function siteName(url: string): string {
  const h = host(url);
  if (/(^|\.)wikipedia\.org$/.test(h)) return "Wikipedia";
  return h;
}

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MONTHS_MLA = ["Jan.", "Feb.", "Mar.", "Apr.", "May", "June", "July", "Aug.", "Sept.", "Oct.", "Nov.", "Dec."];

function end(s: string): string {
  return /[.?!]$/.test(s) ? s : `${s}.`;
}

function doiUrl(doi: string | null | undefined): string | null {
  return doi ? `https://doi.org/${doi.replace(/^https?:\/\/(dx\.)?doi\.org\//, "")}` : null;
}

/** Cite key for BibTeX: lastname + year + first title word. */
function bibKey(s: Source): string {
  const last = s.meta?.authors[0] ? splitName(s.meta.authors[0]).last : siteName(s.url) || "source";
  const word = (s.title.match(/[A-Za-z]{3,}/) ?? ["ref"])[0];
  return `${last}${s.meta?.year ?? ""}${word}`.toLowerCase().replace(/[^a-z0-9]/g, "");
}

const bibEscape = (t: string) => t.replace(/([&%$#_{}])/g, "\\$1");

/** One source in `style`. `read` is when BYTE read it (web pages' access date). */
export function cite(s: Source, style: CiteStyle, read: Date = new Date()): string {
  const m = s.meta;
  const title = s.title.trim() || host(s.url);
  const link = doiUrl(m?.doi) ?? s.url;
  const year = m?.year ?? null;
  const names = (m?.authors ?? []).map(splitName);
  const d = read.getDate();
  const dateLong = `${MONTHS[read.getMonth()]} ${d}, ${read.getFullYear()}`;

  if (style === "bibtex") {
    const fields: [string, string | null | undefined][] = [
      ["title", `{${bibEscape(title)}}`],
      ["author", names.length ? names.map(lastFirst).join(" and ") : null],
      ["year", year ? String(year) : null],
      [m ? (m.venue === "arXiv" ? "howpublished" : "journal") : "howpublished", m ? bibEscape(m.venue) || null : siteName(s.url)],
      ["doi", m?.doi],
      ["url", link],
      ["note", m ? null : `Accessed ${read.toISOString().slice(0, 10)}`],
    ];
    const body = fields
      .filter(([, v]) => v)
      .map(([k, v]) => `  ${k} = {${v}}`)
      .join(",\n");
    return `@${m && m.venue !== "arXiv" ? "article" : "misc"}{${bibKey(s)},\n${body}\n}`;
  }

  if (!m) {
    // A web page.
    const site = siteName(s.url);
    switch (style) {
      case "apa":
        return `${title}. (n.d.). ${site ? `${site}. ` : ""}Retrieved ${dateLong}, from ${s.url}`;
      case "mla":
        return `“${end(title)}” ${site ? `*${site}*, ` : ""}${s.url.replace(/^https?:\/\//, "")}. Accessed ${d} ${MONTHS_MLA[read.getMonth()]} ${read.getFullYear()}.`;
      case "chicago":
        return `${site ? `${site}. ` : ""}“${end(title)}” Accessed ${dateLong}. ${s.url}.`;
      case "harvard":
        return `${site || title} (n.d.) *${title}*. Available at: ${s.url} (Accessed: ${d} ${MONTHS[read.getMonth()]} ${read.getFullYear()}).`;
      case "ieee":
        return `[${s.n}] “${title},” ${site ? `${site}. ` : ""}[Online]. Available: ${s.url} (accessed ${MONTHS_MLA[read.getMonth()].replace(".", "")}. ${d}, ${read.getFullYear()}).`;
    }
  }

  // A paper.
  const venue = m!.venue;
  const y = year ?? "n.d.";
  switch (style) {
    case "apa": {
      // APA 7: up to 20 authors, "&" before the last.
      const list = names.slice(0, 20).map(lastInitials);
      const who = list.length > 1 ? `${list.slice(0, -1).join(", ")}, & ${list[list.length - 1]}` : list[0];
      return `${who ? `${end(who)} ` : ""}(${y}). ${end(title)}${venue ? ` *${venue}*.` : ""} ${link}`;
    }
    case "mla": {
      const who =
        names.length === 0 ? "" : names.length === 1 ? lastFirst(names[0]) : names.length === 2 ? `${lastFirst(names[0])}, and ${firstLast(names[1])}` : `${lastFirst(names[0])}, et al`;
      return `${who ? `${end(who)} ` : ""}“${end(title)}” ${venue ? `*${venue}*, ` : ""}${y}, ${link.replace(/^https?:\/\//, "")}.`;
    }
    case "chicago": {
      const list = names.length > 10 ? [...names.slice(0, 7).map((n, i) => (i ? firstLast(n) : lastFirst(n))), "et al."] : names.map((n, i) => (i ? firstLast(n) : lastFirst(n)));
      const who = names.length > 10 ? list.join(", ") : joinList(list, "and", true);
      return `${who ? `${end(who)} ` : ""}${y}. “${end(title)}” ${venue ? `*${venue}*. ` : ""}${link}.`;
    }
    case "harvard": {
      const list = names.map(lastInitials);
      const who = names.length > 3 ? `${list[0]} et al.` : joinList(list, "and", false);
      return `${who ? `${who} ` : ""}(${y}) ‘${title}’${venue ? `, *${venue}*` : ""}. Available at: ${link}.`;
    }
    case "ieee": {
      const list = names.map(initialsLast);
      const who = names.length > 6 ? `${list[0]} et al.` : joinList(list, "and", names.length > 2);
      return `[${s.n}] ${who ? `${who}, ` : ""}“${title},” ${venue ? `*${venue}*, ` : ""}${y}${m!.doi ? `, doi: ${m!.doi}` : ""}.`;
    }
  }
  return title;
}

/** Every source in one style, for "Copy all". */
export function citeAll(sources: Source[], style: CiteStyle, read?: Date): string {
  const sorted = [...sources].sort((a, b) => a.n - b.n);
  return sorted.map((s) => cite(s, style, read)).join(style === "bibtex" ? "\n\n" : "\n");
}

/** Plain text for the clipboard (the *italics* markers are for display only). */
export const plainCitation = (c: string) => c.replace(/\*([^*]+)\*/g, "$1");
