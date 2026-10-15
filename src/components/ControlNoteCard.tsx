import { listen } from "@tauri-apps/api/event";
import { X } from "lucide-react";
import { useEffect, useState } from "react";

import { errorText, inTauri } from "../lib/api";
import { planApi } from "../lib/control";

interface Offer {
  kind: "failed" | "learned";
  goal: string;
  app: string;
  text: string;
  intro: string;
}

/**
 * Offers to save a note after a PC-control job that failed or was stopped (and, with "Learn from corrections", after the person
 * moved on to another program). Never saves by itself: the person reads the words, can change them, and presses Save.
 */
export function ControlNoteCard() {
  const [offer, setOffer] = useState<Offer | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!inTauri) return;
    const a = listen<{ goal: string; stopped: boolean; app: string }>("control://failed", (e) =>
      setOffer({ kind: "failed", goal: e.payload.goal, app: e.payload.app, text: "", intro: e.payload.stopped ? "You stopped that job." : "That job did not work." }),
    );
    const b = listen<{ goal: string; app: string; title: string }>("control://learned", (e) =>
      setOffer({ kind: "learned", goal: e.payload.goal, app: e.payload.app, text: `use ${e.payload.app}`, intro: `You carried on in ${e.payload.app}.` }),
    );
    return () => {
      void a.then((f) => f());
      void b.then((f) => f());
    };
  }, []);
  if (!offer) return null;
  const save = async () => {
    setError(null);
    try {
      await planApi.noteAdd(offer.app, offer.goal, offer.text);
      setOffer(null);
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <div role="dialog" aria-label="Save a note" style={{ position: "fixed", right: 12, bottom: 64, zIndex: 60, width: 340, maxWidth: "calc(100vw - 24px)", padding: 12, borderRadius: 10, background: "var(--panel)", color: "var(--text)", border: "1px solid var(--accent)" }}>
      <div className="row" style={{ gap: 8 }}>
        <strong className="grow">{offer.intro} Save a note?</strong>
        <button className="icon-btn" aria-label="No thanks" onClick={() => setOffer(null)}>
          <X size={14} />
        </button>
      </div>
      <small style={{ display: "block", margin: "6px 0" }}>“{offer.goal}” — a note helps BYTE next time. It is kept on this PC only.</small>
      <input value={offer.app} onChange={(e) => setOffer({ ...offer, app: e.target.value })} placeholder="Program (notepad)" aria-label="Program" style={{ width: "100%", marginBottom: 6 }} />
      <input value={offer.text} onChange={(e) => setOffer({ ...offer, text: e.target.value })} placeholder="What should BYTE know? (the font is under the gear)" aria-label="What should BYTE know" style={{ width: "100%" }} />
      {error && <small className="error">{error}</small>}
      <div className="row" style={{ gap: 6, marginTop: 8, justifyContent: "flex-end" }}>
        <button className="btn sm" onClick={() => setOffer(null)}>
          No thanks
        </button>
        <button className="btn sm primary" disabled={!offer.app.trim() || !offer.text.trim()} onClick={() => void save()}>
          Save note
        </button>
      </div>
    </div>
  );
}
