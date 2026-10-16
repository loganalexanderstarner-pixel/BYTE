// Every form control the foundations screens draw has a name a screen reader can say (aria-label), and every dialog is named.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const FILES = ["found/FoundOverlays.tsx", "found/FoundSettings.tsx", "found/FoundToggles.tsx", "onboarding/FirstRunFeatures.tsx"];

function tags(text: string, name: string): string[] {
  const out: string[] = [];
  const re = new RegExp(`<${name}\\b`, "g");
  for (let m = re.exec(text); m; m = re.exec(text)) {
    let depth = 0;
    let i = m.index;
    for (; i < text.length; i++) {
      const c = text[i];
      if (c === "{") depth++;
      else if (c === "}") depth--;
      else if (c === ">" && depth === 0) break;
    }
    out.push(text.slice(m.index, i + 1));
  }
  return out;
}

describe("the foundations screens", () => {
  for (const f of FILES) {
    const text = readFileSync(join(__dirname, "..", f), "utf8");
    it(`${f}: inputs, selects and text boxes are named`, () => {
      for (const el of ["input", "select", "textarea"]) for (const t of tags(text, el)) expect(t, `${f}: ${t.slice(0, 80)}`).toMatch(/aria-label|aria-labelledby/);
    });
    it(`${f}: dialogs and alerts are named`, () => {
      for (const t of tags(text, "div").filter((x) => /role="(dialog|alertdialog)"/.test(x))) expect(t).toMatch(/aria-labelledby|aria-label=/);
    });
  }
});
