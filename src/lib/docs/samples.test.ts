import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it } from "vitest";

import { SAMPLE } from "./docs.test";
import { renderDoc } from "./render";
import { themeById } from "./spec";

/** Writes sample files for a person (or LibreOffice) to open:
 *   GEN_DOCS_DIR=/tmp/docs npx vitest run src/lib/docs/samples
 * Skipped otherwise. */
const dir = process.env.GEN_DOCS_DIR;

describe.skipIf(!dir)("sample documents", () => {
  it("writes PDF, PPTX and DOCX in every theme", async () => {
    mkdirSync(dir!, { recursive: true });
    for (const theme of ["midnight", "paper"]) {
      for (const kind of ["pdf", "pptx", "docx"] as const) {
        const b64 = await renderDoc(kind, { ...SAMPLE, kind }, themeById(theme), {});
        writeFileSync(join(dir!, `sample-${theme}.${kind}`), Buffer.from(b64, "base64"));
      }
    }
  }, 60_000);
});
