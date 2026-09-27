/**
 * Message versions (edit & branch).
 *
 * When a message is edited or an answer regenerated, the old version and
 * everything after it is kept. The message at that position stores the other
 * versions in `alts` (each a list of messages starting at that position) and
 * its own place among all versions in `version`. Only the visible version is
 * sent to the model.
 */
export interface Versioned {
  id: string;
  alts?: Versioned[][];
  version?: number;
}

function strip<T extends Versioned>(m: T): T {
  const { alts: _a, version: _v, ...rest } = m;
  return rest as T;
}

/** Every version of the thread from index `i` on, in order. */
export function versionsAt<T extends Versioned>(messages: T[], i: number): T[][] {
  const m = messages[i];
  const alts = (m.alts ?? []) as T[][];
  const current = [strip(m), ...messages.slice(i + 1)];
  const at = Math.min(m.version ?? alts.length, alts.length);
  const out = [...alts];
  out.splice(at, 0, current);
  return out;
}

/** How many versions exist at index `i`, and which one is shown (0-based). */
export function versionInfo(m: Versioned): { count: number; index: number } {
  const count = (m.alts?.length ?? 0) + 1;
  return { count, index: Math.min(m.version ?? count - 1, count - 1) };
}

/** Shows version `target` of the thread from index `i`. */
export function switchVersion<T extends Versioned>(messages: T[], i: number, target: number): T[] {
  const all = versionsAt(messages, i);
  const t = Math.max(0, Math.min(target, all.length - 1));
  const chosen = all[t];
  const rest = all.filter((_, k) => k !== t);
  return [...messages.slice(0, i), { ...chosen[0], alts: rest, version: t }, ...chosen.slice(1)];
}

/** Adds `replacement` as the newest version at index `i` (older ones are kept). */
export function branchAt<T extends Versioned>(messages: T[], i: number, replacement: T): T[] {
  if (i >= messages.length) return [...messages, replacement];
  const all = versionsAt(messages, i);
  return [...messages.slice(0, i), { ...strip(replacement), alts: all, version: all.length }];
}
