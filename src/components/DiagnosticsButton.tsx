import { ClipboardCopy } from "lucide-react";
import { useState } from "react";

import { api } from "../lib/api";
import { copyDiagnostics } from "../lib/diagnostics";

/** Copies what is going on (device, memory, engine, recent log; no chats, no keys) to paste into a chat. */
export function DiagnosticsButton({ label = "Copy diagnostics" }: { label?: string }) {
  const [state, setState] = useState<"idle" | "copied" | "manual" | "failed">("idle");
  const [text, setText] = useState("");

  const run = async () => {
    const r = await copyDiagnostics(api.diagnosticsReport, (t) => navigator.clipboard.writeText(t));
    setText(r.text);
    setState(r.ok ? "copied" : r.text ? "manual" : "failed");
    if (r.ok) setTimeout(() => setState("idle"), 4000);
  };

  return (
    <>
      <button className="btn sm" onClick={() => void run()} title="Copies device, memory and engine details. Nothing from your chats and no keys.">
        <ClipboardCopy size={14} /> {state === "copied" ? "Copied. Paste it to Claude" : label}
      </button>
      {state === "failed" && <small className="muted">Couldn't build the report.</small>}
      {state === "manual" && (
        <textarea className="text-input" readOnly rows={6} value={text} onFocus={(e) => e.currentTarget.select()} aria-label="Diagnostics to copy by hand" style={{ width: "100%", marginTop: 8 }} />
      )}
    </>
  );
}
