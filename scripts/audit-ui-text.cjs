#!/usr/bin/env node
// Audit for mass edits of UI text: the text a person SEES must be identical before and after.
//
//   node scripts/audit-ui-text.cjs <git-ref-before>
//
// Extracts every string literal, template fragment and JSX text node from each changed .ts/.tsx
// file, at the old ref and now, whitespace-normalised, and reports any text that appeared or
// vanished. Import paths are ignored. Written after a mass rewrite of Mac wording into osText()
// corrupted two descriptions ("WhatosText('s taking up space"): osText is the identity on a Mac, so
// the Mac text was supposed to be untouched, and "it should be" was not a check. A regex pass had
// taken an apostrophe in JSX text for a quote. Nothing failed to compile; only this comparison saw it.
//
// Expected differences: an HTML entity decoded in JSX text (&amp; shown as &) displays identically.
// For each file changed by the sweep: the text a person would SEE on a Mac must be identical
// before and after, because osText is the identity there. Compares the multiset of all string
// literals and JSX text nodes (whitespace-normalised) between the old and the new version.
const ts = require("typescript"), cp = require("child_process"), fs = require("fs");
const OLD = process.argv[2];
const files = cp.execSync(`git diff --name-only ${OLD} -- src`, { encoding: "utf8" }).split("\n").filter((f) => /\.tsx?$/.test(f));
const norm = (s) => s.replace(/\s+/g, " ").trim();
function texts(code, name) {
  const sf = ts.createSourceFile(name, code, ts.ScriptTarget.Latest, true, name.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
  const out = [];
  (function v(n) {
    if ((n.kind === ts.SyntaxKind.StringLiteral || n.kind === ts.SyntaxKind.NoSubstitutionTemplateLiteral) && !(n.parent && (n.parent.kind === ts.SyntaxKind.ImportDeclaration || n.parent.kind === ts.SyntaxKind.ExportDeclaration))) { const t = norm(n.text); if (t) out.push(t); }
    else if (n.kind === ts.SyntaxKind.JsxText) { const t = norm(n.getText(sf)); if (t) out.push(t); }
    else if (n.kind === ts.SyntaxKind.TemplateHead || n.kind === ts.SyntaxKind.TemplateMiddle || n.kind === ts.SyntaxKind.TemplateTail) { const t = norm(n.text); if (t) out.push(t); }
    ts.forEachChild(n, v);
  })(sf);
  return out;
}
let bad = 0;
for (const f of files) {
  let before; try { before = cp.execSync(`git show ${OLD}:${f}`, { encoding: "utf8", stdio: ["pipe","pipe","ignore"] }); } catch { continue; }
  const a = texts(before, f), b = texts(fs.readFileSync(f, "utf8"), f);
  const count = (arr) => arr.reduce((m, x) => (m.set(x, (m.get(x) || 0) + 1), m), new Map());
  const A = count(a), B = count(b);
  const gone = [], appeared = [];
  for (const [k, v] of A) if ((B.get(k) || 0) < v) gone.push(k);
  for (const [k, v] of B) if ((A.get(k) || 0) < v) appeared.push(k);
  if (gone.length || appeared.length) {
    bad++; console.log(`\n### ${f}`);
    gone.slice(0, 4).forEach((x) => console.log("   WAS : " + x.slice(0, 130)));
    appeared.slice(0, 4).forEach((x) => console.log("   NOW : " + x.slice(0, 130)));
  }
}
console.log(`\n${files.length} files compared against ${OLD}; ${bad} with different displayed text`);
if (files.length === 0) { console.log("WARNING: nothing was compared. Is the ref right, and is there a change under src/?"); process.exitCode = 2; }
