/** Helpers for the browser's allow/block site lists (Settings, Browser). */

/** A domain from what the person typed: no address start, path, port or "www.". Empty when it isn't one. */
export function cleanSite(raw: string): string {
  let s = raw.trim().toLowerCase().replace(/^[a-z]+:\/\//, "");
  s = s.split(/[/?#]/)[0].replace(/^.*@/, "").replace(/:\d+$/, "");
  s = s.replace(/^\*\./, "").replace(/^www\./, "").replace(/^\.+|\.+$/g, "");
  return /^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(s) ? s : "";
}

/** Adds a site to a list (cleaned, no duplicates). */
export function addSite(list: string[], raw: string): string[] {
  const s = cleanSite(raw);
  return s && !list.includes(s) ? [...list, s] : list;
}

export function removeSite(list: string[], site: string): string[] {
  return list.filter((x) => x !== site);
}

/** Most steps the person can ask for (the Rust side caps at 100). */
export function clampSteps(n: number): number {
  return Number.isFinite(n) ? Math.min(100, Math.max(0, Math.round(n))) : 0;
}
