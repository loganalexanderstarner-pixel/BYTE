/** Type-ahead for the settings search: the settings whose name starts with what was typed come first, then those with a word that does. */
export function suggest(titles: string[], typed: string, max = 8): string[] {
  const q = typed.trim().toLowerCase();
  if (!q) return [];
  const seen = new Set<string>();
  const starts: string[] = [];
  const words: string[] = [];
  for (const raw of titles) {
    const title = raw.replace(/\s+/g, " ").trim();
    const low = title.toLowerCase();
    if (!title || title.length > 80 || seen.has(low) || low === q) continue;
    seen.add(low);
    if (low.startsWith(q)) starts.push(title);
    else if (low.split(/[^a-z0-9']+/).some((w, i) => i > 0 && w.startsWith(q.split(/\s+/).pop() ?? q)) && q.split(/\s+/).length === 1) words.push(title);
  }
  return [...starts, ...words].slice(0, max);
}

/** A setting's name as shown: the first text of its label, without the small explanation under it. */
export function fieldTitle(field: Element): string {
  const span = field.querySelector("span");
  if (!span) return "";
  let out = "";
  for (const n of Array.from(span.childNodes)) {
    if (n.nodeType === 3) out += n.textContent ?? "";
    else if ((n as Element).tagName !== "SMALL" && (n as Element).tagName !== "BR") out += (n as Element).textContent ?? "";
  }
  return out.replace(/\s+/g, " ").trim();
}
