// Every button must have a name a screen reader (VoiceOver) can say: visible text,
// aria-label, aria-labelledby or title. Icon-only buttons are the usual miss, so
// this walks every .tsx file with the TypeScript parser and lists the ones without.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const ROOT = join(__dirname, "..");
/** Only the UI's own sources (src/): walking the repo root also read node_modules and the Rust build folder. */
const SRC = __dirname;
const SKIP = new Set(["node_modules", "dist", "target"]);

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    if (SKIP.has(n)) return [];
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return files(p);
    return p.endsWith(".tsx") && !p.endsWith(".test.tsx") ? [p] : [];
  });
}

const NAMED = new Set(["aria-label", "aria-labelledby", "title"]);

/** True when the children can give the button a name (text, or an expression that may be text). */
function hasText(children: ts.NodeArray<ts.JsxChild>): boolean {
  return children.some((c) => {
    if (ts.isJsxText(c)) return c.text.trim().length > 0;
    if (ts.isJsxExpression(c)) return !!c.expression;
    if (ts.isJsxElement(c)) return hasText(c.children);
    return false; // <Icon /> alone says nothing
  });
}

function unnamedButtons(file: string, text = readFileSync(file, "utf8")): string[] {
  const src = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const out: string[] = [];
  const visit = (n: ts.Node) => {
    const check = (tag: ts.JsxTagNameExpression, attrs: ts.JsxAttributes, children: ts.NodeArray<ts.JsxChild> | null) => {
      if (tag.getText(src) !== "button") return;
      const named = attrs.properties.some((p) => ts.isJsxSpreadAttribute(p) || NAMED.has(p.name.getText(src)));
      if (!named && !(children && hasText(children))) {
        const { line } = src.getLineAndCharacterOfPosition(n.getStart(src));
        out.push(`${relative(ROOT, file)}:${line + 1}`);
      }
    };
    if (ts.isJsxElement(n)) check(n.openingElement.tagName, n.openingElement.attributes, n.children);
    else if (ts.isJsxSelfClosingElement(n)) check(n.tagName, n.attributes, null);
    ts.forEachChild(n, visit);
  };
  visit(src);
  return out;
}

describe("accessibility", () => {
  it("catches an icon-only button and accepts named ones", () => {
    const code = `const A = () => (<div>
      <button onClick={go}><X size={14} /></button>
      <button aria-label="Close"><X /></button>
      <button title="Close"><X /></button>
      <button>Save</button>
      <button>{label}</button>
      <button><X /> <span>Save</span></button>
    </div>);`;
    expect(unnamedButtons(join(ROOT, "x.tsx"), code)).toEqual(["x.tsx:2"]);
  });

  it("every button has a name VoiceOver can read", () => {
    const missing = files(SRC).flatMap((f) => unnamedButtons(f));
    expect(missing).toEqual([]);
    // Parses every component file in src/ (about a second).
  }, 30_000);
});
