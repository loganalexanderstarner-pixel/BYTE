/** "Copy diagnostics": the report (Rust, no chats or keys) goes to the clipboard to paste into a chat. */

export type DiagnosticsResult = { ok: true; text: string } | { ok: false; text: string; reason: string };

/**
 * Fetches the report and copies it. When the clipboard refuses (some web views
 * do outside a tap), the text is still returned so the screen can show it to copy by hand.
 */
export async function copyDiagnostics(fetchReport: () => Promise<string>, write: (text: string) => Promise<void>): Promise<DiagnosticsResult> {
  let text = "";
  try {
    text = await fetchReport();
  } catch (e) {
    return { ok: false, text: "", reason: e instanceof Error ? e.message : String(e) };
  }
  try {
    await write(text);
    return { ok: true, text };
  } catch (e) {
    return { ok: false, text, reason: e instanceof Error ? e.message : String(e) };
  }
}
