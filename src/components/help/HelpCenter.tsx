import { ArrowLeft, LifeBuoy, Search, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";

import { availableExamples } from "../../lib/examples";
import { ARTICLES, linkTarget, searchHelp } from "../../lib/help";
import { renderMarkdown } from "../../lib/markdown";
import { canSpeak, useStore, type SettingsTab } from "../../state/store";

/** The help center (?, ⌘?): short offline articles, search, and example prompts to try. */
export function HelpCenter() {
  const start = useStore((s) => s.help) ?? undefined;
  const onClose = useStore((s) => s.closeHelp);
  const settings = useStore((s) => s.settings);
  const openSettings = useStore((s) => s.openSettings);
  const send = useStore((s) => s.send);
  const newChat = useStore((s) => s.newChat);
  const [query, setQuery] = useState("");
  const [id, setId] = useState<string>(start ?? "getting-started");
  const body = useRef<HTMLDivElement>(null);
  const found = useMemo(() => searchHelp(query), [query]);
  const article = id === "ideas" ? null : (ARTICLES.find((a) => a.id === id) ?? ARTICLES[0]);
  const examples = useMemo(
    () => availableExamples(settings as unknown as Record<string, unknown>, { web: settings?.webSearch !== false, mac: canSpeak() }),
    [settings],
  );
  const groups = useMemo(() => [...new Set(examples.map((e) => e.group))], [examples]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  useEffect(() => body.current?.scrollTo(0, 0), [id]);

  // Links inside an article: other articles, a Settings tab, or the web (opened outside).
  const onClick = (e: MouseEvent) => {
    const a = (e.target as HTMLElement).closest("a");
    const href = a?.getAttribute("href");
    if (!href) return;
    const t = linkTarget(href);
    if (!t) return;
    e.preventDefault();
    if (t.kind === "help") setId(t.id);
    else {
      onClose();
      openSettings(t.tab as SettingsTab);
    }
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="writing-panel help-panel" role="dialog" aria-modal="true" aria-label="Help">
        <div className="recipe-box-head">
          <LifeBuoy size={18} />
          <h2>Help</h2>
          <span className="faint small grow">Works offline</span>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>
        <div className="notes-body">
          <div className="notes-list">
            <label className="search">
              <Search size={14} className="faint" />
              <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search help" aria-label="Search help" autoFocus />
            </label>
            {found.map((a) => (
              <button key={a.id} className={`note-row ${id === a.id ? "on" : ""}`} onClick={() => setId(a.id)}>
                <b>{a.title}</b>
              </button>
            ))}
            {!query && (
              <button className={`note-row ${id === "ideas" ? "on" : ""}`} onClick={() => setId("ideas")}>
                <b>Ideas to try</b>
                <span className="faint small">{examples.length} things to ask</span>
              </button>
            )}
            {query && !found.length && <p className="faint small" style={{ padding: 8 }}>Nothing matches. Try other words, or just ask BYTE.</p>}
          </div>
          <div className="notes-editor" ref={body}>
            {article ? (
              <>
                {id !== "getting-started" && (
                  <button className="linklike faint small" onClick={() => setId("getting-started")}>
                    <ArrowLeft size={12} /> Getting started
                  </button>
                )}
                <h2 style={{ margin: "4px 0" }}>{article.title}</h2>
                <div className="note-preview markdown help-article" onClick={onClick} dangerouslySetInnerHTML={{ __html: renderMarkdown(article.body) }} />
              </>
            ) : (
              <div className="note-preview help-article">
                <h2 style={{ marginTop: 0 }}>Ideas to try</h2>
                <p className="faint small">Click one to ask it in a new chat. Only ideas that work with your settings are shown.</p>
                {groups.map((g) => (
                  <div key={g}>
                    <h4>{g}</h4>
                    <div className="chips">
                      {examples
                        .filter((e) => e.group === g)
                        .map((e) => (
                          <button
                            key={e.text}
                            className="chip"
                            onClick={() => {
                              onClose();
                              newChat();
                              if (e.text.includes("…")) useStore.setState({ prefill: e.text.replace("…", "") });
                              else void send(e.text);
                            }}
                          >
                            {e.text}
                          </button>
                        ))}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
