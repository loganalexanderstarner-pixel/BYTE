import { Download, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { autoApi, type AuditEntry } from "../../lib/auto";
import { isWindows } from "../../lib/keys";

/** Settings, Control: every step of every autonomous job, with time, program and result. Never what was typed. */
export function AuditPanel() {
  const [rows, setRows] = useState<AuditEntry[]>([]);
  const [note, setNote] = useState<string | null>(null);
  const refresh = useCallback(() => {
    if (inTauri) autoApi.audit(300).then((r) => setRows(r.slice().reverse()), (e) => setNote(errorText(e)));
  }, []);
  useEffect(refresh, [refresh]);
  if (!isWindows()) return null;
  const save = (format: "csv" | "md") => autoApi.auditExport(format).then((p) => setNote(`Saved to ${p}`), (e) => setNote(errorText(e)));
  return (
    <div className="field" style={{ display: "block" }} id="audit-trail">
      <span>
        Audit trail of autonomous jobs
        <small>Every step with its time, program and result. What was typed is never written down, only how much. Kept on this PC.</small>
      </span>
      <div className="row" style={{ gap: 8, margin: "8px 0", flexWrap: "wrap" }}>
        <button className="btn sm" onClick={refresh}>
          <RefreshCw size={14} /> Refresh
        </button>
        <button className="btn sm" disabled={rows.length === 0} onClick={() => void save("csv")}>
          <Download size={14} /> Export (spreadsheet)
        </button>
        <button className="btn sm" disabled={rows.length === 0} onClick={() => void save("md")}>
          <Download size={14} /> Export (text)
        </button>
        <button className="btn sm ghost" disabled={rows.length === 0} onClick={() => void autoApi.auditClear().then(refresh)}>
          <Trash2 size={14} /> Clear
        </button>
      </div>
      {note && <small>{note}</small>}
      {rows.length === 0 ? (
        <small>Nothing yet.</small>
      ) : (
        <ul className="plain-list" aria-label="Audit trail" style={{ maxHeight: 240, overflow: "auto" }}>
          {rows.map((e, i) => (
            <li key={`${e.at}-${i}`}>
              <small>
                {e.at} · {e.ok ? "✓" : "✗"} {e.app ? `${e.app}: ` : ""}
                {e.what}
                {e.result ? ` — ${e.result}` : ""} · {e.stage}
              </small>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
