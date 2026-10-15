import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { documentDir, join } from "@tauri-apps/api/path";
import { ArrowLeft, Download, Eye, GraduationCap, List, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { GRADES, intervalText, nextInterval } from "../../lib/study";
import type { DeckSummary, StudyCard } from "../../lib/types";
import { useStore } from "../../state/store";

type View = { kind: "decks" } | { kind: "study"; deck: DeckSummary } | { kind: "cards"; deck: DeckSummary };

/** Study sessions: one card at a time, flip, grade; keys Space to flip, 1–4 to grade. */
function Session({ deck, onDone }: { deck: DeckSummary; onDone: () => void }) {
  const [queue, setQueue] = useState<StudyCard[] | null>(null);
  const [shown, setShown] = useState(false);
  const [done, setDone] = useState(0);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.studyQueue(deck.id).then(setQueue).catch((e) => setError(errorText(e)));
  }, [deck.id]);
  const card = queue?.[0];
  const grade = useCallback(
    async (g: number) => {
      if (!card) return;
      try {
        await api.cardReview(card.id, g);
        setDone((d) => d + 1);
        setShown(false);
        // "Again" comes back at the end of this session.
        setQueue((q) => (q ? (g < 3 ? [...q.slice(1), { ...card, reps: 0, interval: 1 }] : q.slice(1)) : q));
      } catch (e) {
        setError(errorText(e));
      }
    },
    [card],
  );
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === " " && !shown) {
        e.preventDefault();
        setShown(true);
      } else if (shown) {
        const g = GRADES.find((x) => x.key === e.key);
        if (g) void grade(g.grade);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shown, grade]);

  if (error) return <div className="banner danger">{error}</div>;
  if (!queue) return <p className="muted">Loading…</p>;
  if (!card)
    return (
      <div className="study-done">
        <GraduationCap size={28} />
        <h3>{done ? `Done for now: ${done} card${done === 1 ? "" : "s"} reviewed` : "Nothing due right now"}</h3>
        <p className="muted">BYTE brings each card back just before you'd forget it. Come back tomorrow.</p>
        <button className="btn sm" onClick={onDone}>
          Back to decks
        </button>
      </div>
    );
  return (
    <div className="session">
      <div className="session-count muted">
        {queue.length} left · {done} done
      </div>
      <div className={`session-card ${shown ? "shown" : ""}`}>
        <div className="session-front">{card.front}</div>
        {shown && <div className="session-back">{card.back}</div>}
      </div>
      {!shown ? (
        <button className="btn primary" onClick={() => setShown(true)}>
          <Eye size={14} /> Show answer <kbd>Space</kbd>
        </button>
      ) : (
        <div className="grade-row">
          {GRADES.map((g) => (
            <button key={g.grade} className={`btn grade g${g.grade}`} onClick={() => void grade(g.grade)}>
              <b>{g.label}</b>
              <small>{intervalText(nextInterval(card, g.grade))}</small>
              <kbd>{g.key}</kbd>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function CardList({ deck }: { deck: DeckSummary }) {
  const [cards, setCards] = useState<StudyCard[] | null>(null);
  const load = useCallback(() => void api.deckCards(deck.id).then(setCards), [deck.id]);
  useEffect(load, [load]);
  return (
    <ul className="deck-cards">
      {(cards ?? []).map((c) => (
        <li key={c.id}>
          <div>
            <b>{c.front}</b>
            <p className="muted">{c.back}</p>
          </div>
          <span className="muted small">{c.reps + c.lapses === 0 ? "new" : `every ${intervalText(c.interval)}`}</span>
          <button className="icon-btn" aria-label="Delete card" onClick={() => void api.cardDelete(c.id).then(load)}>
            <Trash2 size={14} />
          </button>
        </li>
      ))}
    </ul>
  );
}

/** The Study panel: decks with what's due, study sessions, card lists, Anki export. */
export function StudyPanel() {
  const study = useStore((s) => s.study);
  const close = useStore((s) => s.closeStudy);
  const [decks, setDecks] = useState<DeckSummary[] | null>(null);
  const [view, setView] = useState<View>({ kind: "decks" });
  const [status, setStatus] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.decksList();
      setDecks(list);
      return list;
    } catch (e) {
      setStatus(errorText(e));
      return [];
    }
  }, []);
  useEffect(() => {
    void load().then((list) => {
      const d = list.find((x) => x.id === study?.deck);
      if (d) setView({ kind: "study", deck: d });
    });
  }, [load, study?.deck]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  const exportDeck = async (d: DeckSummary) => {
    try {
      const text = await api.deckExport(d.id);
      const dest = await saveDialog({ defaultPath: await join(await documentDir(), "BYTE", `${d.name.replace(/[\\/:*?"<>|]+/g, " ").trim() || "Deck"} (Anki).txt`) });
      if (!dest) return;
      await api.docSave(dest, btoa(unescape(encodeURIComponent(text))));
      setStatus("Saved. In Anki: File → Import, and pick this file.");
    } catch (e) {
      setStatus(errorText(e));
    }
  };
  const remove = async (d: DeckSummary) => {
    if (!window.confirm(`Delete the deck “${d.name}” and its ${d.cards} cards?`)) return;
    await api.deckDelete(d.id);
    await load();
  };
  const back = () => {
    setView({ kind: "decks" });
    void load();
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="study-panel" role="dialog" aria-modal="true" aria-label="Study">
        <div className="recipe-box-head">
          {view.kind !== "decks" && (
            <button className="icon-btn" onClick={back} aria-label="Back to decks">
              <ArrowLeft size={18} />
            </button>
          )}
          <GraduationCap size={18} />
          <h2>{view.kind === "decks" ? "Study" : view.deck.name}</h2>
          <span className="spacer" />
          <button className="icon-btn" onClick={close} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {status && <div className="banner">{status}</div>}
        {view.kind === "study" && <Session deck={view.deck} onDone={back} />}
        {view.kind === "cards" && <CardList deck={view.deck} />}
        {view.kind === "decks" &&
          (decks == null ? (
            <p className="muted">Loading…</p>
          ) : decks.length === 0 ? (
            <div className="empty-box">
              <p>No decks yet.</p>
              <p className="muted">
                Ask BYTE for flashcards (“make flashcards about the French Revolution”, or attach your notes and say “flashcards from this”)
                and press <b>Save deck</b> or <b>Study now</b>.
              </p>
            </div>
          ) : (
            <ul className="decks">
              {decks.map((d) => (
                <li key={d.id}>
                  <div className="deck-info">
                    <b>{d.name}</b>
                    <span className="muted small">
                      {d.cards} cards · <span className={d.due ? "due" : ""}>{d.due} due</span> · {d.new} new
                    </span>
                  </div>
                  <button className="btn sm primary" disabled={d.due + d.new === 0} onClick={() => setView({ kind: "study", deck: d })}>
                    Study
                  </button>
                  <button className="icon-btn" title="Cards" aria-label={`Cards in ${d.name}`} onClick={() => setView({ kind: "cards", deck: d })}>
                    <List size={15} />
                  </button>
                  <button className="icon-btn" title="Export to Anki" aria-label={`Export ${d.name} to Anki`} onClick={() => void exportDeck(d)}>
                    <Download size={15} />
                  </button>
                  <button className="icon-btn" title="Delete deck" aria-label={`Delete ${d.name}`} onClick={() => void remove(d)}>
                    <Trash2 size={15} />
                  </button>
                </li>
              ))}
            </ul>
          ))}
      </div>
    </div>
  );
}
