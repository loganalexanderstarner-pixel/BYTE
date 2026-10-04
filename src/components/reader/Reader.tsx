import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { FolderOpen, Loader2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { locate } from "../../lib/reader";
import { useStore } from "../../state/store";

/** Side panel with a file's text and the cited passage highlighted. */
export function Reader() {
  const doc = useStore((s) => s.reader);
  const close = useStore((s) => s.closeReader);
  const [text, setText] = useState<string | null>(doc?.text ?? null);
  const [error, setError] = useState<string | null>(null);
  const mark = useRef<HTMLElement>(null);

  useEffect(() => {
    setError(null);
    if (!doc) return;
    if (doc.text !== undefined) {
      setText(doc.text);
      return;
    }
    setText(null);
    if (!doc.path || !inTauri) return;
    let alive = true;
    api.fileIngest(doc.path).then(
      (f) => alive && setText(f.text),
      (e) => alive && setError(errorText(e)),
    );
    return () => {
      alive = false;
    };
  }, [doc]);

  useEffect(() => {
    mark.current?.scrollIntoView({ block: "center" });
  }, [text]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  if (!doc) return null;
  const at = text ? locate(text, doc.highlight ?? "", doc.page ?? null) : null;
  return (
    <aside className="reader" aria-label={`Reading ${doc.title}`}>
      <header className="reader-head">
        <div className="grow" style={{ minWidth: 0 }}>
          <div className="title">{doc.title}</div>
          {doc.page != null && <div className="faint">Page {doc.page}</div>}
        </div>
        {doc.path && (
          <button className="icon-btn" onClick={() => void revealItemInDir(doc.path!).catch(() => {})} title="Show in Finder">
            <FolderOpen size={16} />
          </button>
        )}
        <button className="icon-btn" onClick={close} aria-label="Close reader">
          <X size={16} />
        </button>
      </header>
      <div className="reader-body">
        {error && <div className="banner danger">{error}</div>}
        {!text && !error && (
          <p className="faint">
            <Loader2 size={13} className="spin" /> Opening…
          </p>
        )}
        {text && (
          <pre className="reader-text">
            {at ? (
              <>
                {text.slice(0, at[0])}
                <mark ref={mark}>{text.slice(at[0], at[1])}</mark>
                {text.slice(at[1])}
              </>
            ) : (
              text
            )}
          </pre>
        )}
      </div>
    </aside>
  );
}
