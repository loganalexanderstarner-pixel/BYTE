/** The release notes' first paragraph as plain text, for the Updates row (no "**" or link syntax). */
export function updateSummary(notes: string): string {
  const para = notes
    .split(/\n\s*\n/)
    .map((p) => p.trim())
    .find((p) => p && !p.startsWith("#"));
  if (!para) return "";
  return para
    .split("\n")
    .map((l) => l.trim())
    .join(" ")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/\*\*|__|`/g, "")
    .replace(/\s+/g, " ")
    .trim();
}
