// Study helpers: SM-2 previews for the grade buttons (mirrors src-tauri/src/study.rs),
// quiz scoring, and friendly interval text.
import type { StudyCard } from "./types";

export const GRADES = [
  { grade: 1, label: "Again", key: "1" },
  { grade: 3, label: "Hard", key: "2" },
  { grade: 4, label: "Good", key: "3" },
  { grade: 5, label: "Easy", key: "4" },
] as const;

/** Days until the next review if this card gets `grade` now (same maths as Rust `study::review`). */
export function nextInterval(c: Pick<StudyCard, "ease" | "interval" | "reps">, grade: number): number {
  if (grade < 3) return 1;
  const reps = c.reps + 1;
  let interval = reps === 1 ? 1 : reps === 2 ? 6 : Math.round(Math.max(1, c.interval) * c.ease);
  if (grade === 5 && reps > 1) interval = Math.round(interval * 1.3);
  else if (grade === 3 && reps > 2) interval = Math.max(1, Math.round(Math.max(1, c.interval) * 1.2));
  return Math.min(3650, Math.max(1, interval));
}

/** "tomorrow", "6 days", "3 wk", "4 mo", "1.2 yr". */
export function intervalText(days: number): string {
  if (days <= 1) return "tomorrow";
  if (days < 14) return `${days} days`;
  if (days < 60) return `${Math.round(days / 7)} wk`;
  if (days < 365) return `${Math.round(days / 30)} mo`;
  return `${Math.round((days / 365) * 10) / 10} yr`;
}

/** Quiz result: right answers and a percent. */
export function quizScore(answers: (number | null)[], correct: number[]): { right: number; total: number; percent: number } {
  const right = answers.filter((a, i) => a != null && a === correct[i]).length;
  const total = correct.length;
  return { right, total, percent: total ? Math.round((right / total) * 100) : 0 };
}

/** Questions answered wrong, as flashcards (question → right answer + why). */
export function missedAsCards(questions: { question: string; choices: string[]; answer: number; explanation: string }[], answers: (number | null)[]) {
  return questions
    .map((q, i) => ({ q, a: answers[i] }))
    .filter(({ q, a }) => a != null && a !== q.answer)
    .map(({ q }) => ({ front: q.question, back: [q.choices[q.answer], q.explanation].filter(Boolean).join(" — ") }));
}
