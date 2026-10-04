import { Check, GraduationCap, Layers, RotateCcw, Save, X } from "lucide-react";
import { useState } from "react";

import { api, errorText } from "../../lib/api";
import { missedAsCards, quizScore } from "../../lib/study";
import type { Flashcards, Quiz } from "../../lib/types";
import { useStore } from "../../state/store";

/** Flashcards made in chat: flip through them, save as a deck, study with spaced repetition. */
export function FlashcardsCard({ set }: { set: Flashcards }) {
  const [flipped, setFlipped] = useState<Set<number>>(new Set());
  const [deckId, setDeckId] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const openStudy = useStore((s) => s.openStudy);
  const flip = (i: number) => {
    const n = new Set(flipped);
    if (n.has(i)) n.delete(i);
    else n.add(i);
    setFlipped(n);
  };
  const save = async () => {
    try {
      const id = await api.deckSave(set.title || "Flashcards", set.cards);
      setDeckId(id);
      setStatus(`Saved to the deck “${set.title}”`);
      return id;
    } catch (e) {
      setStatus(errorText(e));
      return null;
    }
  };
  const study = async () => {
    const id = deckId ?? (await save());
    if (id != null) openStudy(id);
  };
  return (
    <div className="study-card" role="region" aria-label={`Flashcards: ${set.title}`}>
      <div className="shop-head">
        <Layers size={15} />
        <b>{set.title}</b>
        <span className="muted">· {set.cards.length} cards · tap a card to flip it</span>
      </div>
      <div className="flash-grid">
        {set.cards.map((c, i) => (
          <button key={i} className={`flash ${flipped.has(i) ? "flipped" : ""}`} onClick={() => flip(i)} aria-pressed={flipped.has(i)}>
            <span className="flash-side">{flipped.has(i) ? "Answer" : "Question"}</span>
            {flipped.has(i) ? c.back : c.front}
          </button>
        ))}
      </div>
      <div className="study-actions">
        <button className="btn sm primary" onClick={() => void study()}>
          <GraduationCap size={13} /> Study now
        </button>
        {deckId == null && (
          <button className="btn sm ghost" onClick={() => void save()}>
            <Save size={13} /> Save deck
          </button>
        )}
        {status && <span className="hint ok">{status}</span>}
      </div>
    </div>
  );
}

/** A multiple-choice quiz that scores itself and explains each answer. */
export function QuizCard({ quiz }: { quiz: Quiz }) {
  const [answers, setAnswers] = useState<(number | null)[]>(() => quiz.questions.map(() => null));
  const [done, setDone] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const correct = quiz.questions.map((q) => q.answer);
  const score = quizScore(answers, correct);
  const all = answers.every((a) => a != null);
  const pick = (qi: number, ci: number) => {
    if (done) return;
    setAnswers(answers.map((a, i) => (i === qi ? ci : a)));
  };
  const saveMissed = async () => {
    const cards = missedAsCards(quiz.questions, answers);
    try {
      await api.deckSave(`${quiz.title || "Quiz"}: to review`, cards);
      setStatus(`Saved ${cards.length} card${cards.length === 1 ? "" : "s"} to study`);
    } catch (e) {
      setStatus(errorText(e));
    }
  };
  return (
    <div className="study-card quiz" role="region" aria-label={`Quiz: ${quiz.title}`}>
      <div className="shop-head">
        <GraduationCap size={15} />
        <b>{quiz.title}</b>
        <span className="muted">· {quiz.questions.length} questions</span>
      </div>
      <ol className="quiz-list">
        {quiz.questions.map((q, qi) => (
          <li key={qi}>
            <p className="quiz-q">{q.question}</p>
            <div className="quiz-choices" role="radiogroup" aria-label={`Question ${qi + 1}`}>
              {q.choices.map((c, ci) => {
                const chosen = answers[qi] === ci;
                const state = done ? (ci === q.answer ? "right" : chosen ? "wrong" : "") : chosen ? "chosen" : "";
                return (
                  <button key={ci} role="radio" aria-checked={chosen} className={`quiz-choice ${state}`} onClick={() => pick(qi, ci)} disabled={done}>
                    <span className="letter">{String.fromCharCode(65 + ci)}</span>
                    {c}
                    {done && ci === q.answer && <Check size={13} />}
                    {done && chosen && ci !== q.answer && <X size={13} />}
                  </button>
                );
              })}
            </div>
            {done && q.explanation && <p className="quiz-why muted">{q.explanation}</p>}
          </li>
        ))}
      </ol>
      <div className="study-actions">
        {!done ? (
          <button className="btn sm primary" disabled={!all} onClick={() => setDone(true)} title={all ? "" : "Answer every question first"}>
            <Check size={13} /> Check my answers
          </button>
        ) : (
          <>
            <span className="quiz-score">
              {score.right} / {score.total} · {score.percent}%
            </span>
            <button className="btn sm ghost" onClick={() => { setAnswers(quiz.questions.map(() => null)); setDone(false); setStatus(null); }}>
              <RotateCcw size={13} /> Try again
            </button>
            {score.right < score.total && (
              <button className="btn sm ghost" onClick={() => void saveMissed()}>
                <Save size={13} /> Save the ones I missed as flashcards
              </button>
            )}
          </>
        )}
        {status && <span className="hint ok">{status}</span>}
      </div>
    </div>
  );
}
