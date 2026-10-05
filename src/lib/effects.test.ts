import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/** Every .tsx file under src. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? sources(path) : path.endsWith(".tsx") ? [path] : [];
  });
}

describe("effects", () => {
  // `useEffect(() => el?.scrollTo(0, 0), [id])` hands React whatever scrollTo returns as the effect's
  // cleanup. On Windows' web view that is not undefined, and React then threw "destroy is not a
  // function" when the panel closed, which blanked the whole app (the Help center did exactly this).
  // An effect that is only a call gets braces, so it returns nothing. (An effect that returns a
  // function on purpose, `() => () => flush()`, is a cleanup and is fine.)
  it("never return the value of an expression", () => {
    const offenders: string[] = [];
    for (const file of sources("src")) {
      const text = readFileSync(file, "utf8");
      for (const m of text.matchAll(/useEffect\(\s*\(\)\s*=>\s*(?!\(\)\s*=>)([^\s{][^\n]{0,60})/g)) offenders.push(`${file}: useEffect(() => ${m[1]}`);
    }
    expect(offenders).toEqual([]);
  });
});
