import { openUrl } from "@tauri-apps/plugin-opener";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { documentDir, join } from "@tauri-apps/api/path";
import { BookmarkCheck, BookmarkPlus, ChefHat, Clock, Copy, Check, FileDown, Minus, Plus, Timer, Users } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../../lib/api";
import { DOC_THEMES, fileName } from "../../lib/docs/spec";
import { ingredientLine, minutesText, recipeDocSpec, recipeEmoji, recipeText, totalMinutes } from "../../lib/recipe";
import type { Recipe } from "../../lib/types";

/** A countdown for one step; announces when done. */
function StepTimer({ minutes }: { minutes: number }) {
  const [left, setLeft] = useState<number | null>(null);
  useEffect(() => {
    if (left == null || left <= 0) return;
    const t = setTimeout(() => setLeft(left - 1), 1000);
    return () => clearTimeout(t);
  }, [left]);
  const running = left != null && left > 0;
  const done = left === 0;
  const mm = left != null ? `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}` : `${minutes} min`;
  return (
    <button
      className={`step-timer ${running ? "running" : ""} ${done ? "done" : ""}`}
      onClick={() => setLeft(running ? null : minutes * 60)}
      title={running ? "Stop the timer" : `Start a ${minutes}-minute timer`}
      aria-live={done ? "assertive" : "off"}
    >
      <Timer size={12} /> {done ? "Time's up" : mm}
    </button>
  );
}

/**
 * A recipe as a chef would hand it to you: photo, servings you can change,
 * ingredients to tick off (what you have is marked), steps with timers and
 * doneness cues, tips, swaps. Save to the recipe box, copy, or save as a PDF.
 */
export function RecipeCard({ recipe, savedId, onDelete }: { recipe: Recipe; savedId?: number; onDelete?: () => void }) {
  const [servings, setServings] = useState(recipe.servings);
  const [got, setGot] = useState<Set<number>>(() => new Set(recipe.ingredients.flatMap((i, k) => (i.have ? [k] : []))));
  const [doneSteps, setDoneSteps] = useState<Set<number>>(new Set());
  const [saved, setSaved] = useState<number | null>(savedId ?? null);
  const [copied, setCopied] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [imgOk, setImgOk] = useState(!!recipe.image);
  const total = totalMinutes(recipe);
  const toggle = (set: Set<number>, k: number) => {
    const n = new Set(set);
    if (n.has(k)) n.delete(k);
    else n.add(k);
    return n;
  };

  const save = async () => {
    try {
      setSaved(await api.recipeSave(recipe));
      setStatus("Saved to your recipe box");
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };
  const copy = () =>
    void navigator.clipboard?.writeText(recipeText(recipe, servings)).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  const pdf = async () => {
    try {
      const spec = recipeDocSpec(recipe, servings);
      const dest = await saveDialog({ defaultPath: await join(await documentDir(), "BYTE", fileName(recipe.title, "pdf")) });
      if (!dest) return;
      const { renderDoc } = await import("../../lib/docs/render");
      await api.docSave(dest, await renderDoc("pdf", spec, DOC_THEMES[0], {}));
      setStatus("Saved the PDF");
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <article className="recipe" aria-label={`Recipe: ${recipe.title}`}>
      <div className="recipe-hero">
        {imgOk ? (
          <img src={recipe.image} alt={recipe.title} loading="lazy" referrerPolicy="no-referrer" onError={() => setImgOk(false)} />
        ) : (
          <div className="recipe-emoji" aria-hidden>
            {recipeEmoji(recipe)}
          </div>
        )}
        <div className="recipe-head">
          <div className="recipe-kicker">
            <ChefHat size={13} /> {[recipe.category, recipe.cuisine].filter(Boolean).join(" · ") || "Recipe"}
          </div>
          <h3>{recipe.title}</h3>
          <p>{recipe.description}</p>
          <div className="recipe-meta">
            {total && (
              <span>
                <Clock size={13} /> {minutesText(total)}
                {recipe.prepMin ? ` (${minutesText(recipe.prepMin)} prep)` : ""}
              </span>
            )}
            {recipe.difficulty && <span className="chip">{recipe.difficulty}</span>}
            <span className="servings" aria-label="Servings">
              <Users size={13} />
              <button className="icon-btn" onClick={() => setServings(Math.max(1, servings - 1))} aria-label="Fewer servings">
                <Minus size={12} />
              </button>
              {servings}
              <button className="icon-btn" onClick={() => setServings(Math.min(48, servings + 1))} aria-label="More servings">
                <Plus size={12} />
              </button>
            </span>
          </div>
          <div className="recipe-actions">
            {onDelete ? (
              <button className="btn sm ghost danger" onClick={onDelete}>
                Delete
              </button>
            ) : (
              <button className={`btn sm ${saved ? "" : "primary"}`} onClick={() => void save()} disabled={!!saved}>
                {saved ? <BookmarkCheck size={13} /> : <BookmarkPlus size={13} />} {saved ? "Saved" : "Save recipe"}
              </button>
            )}
            <button className="btn sm ghost" onClick={copy}>
              {copied ? <Check size={13} /> : <Copy size={13} />} Copy
            </button>
            <button className="btn sm ghost" onClick={() => void pdf()}>
              <FileDown size={13} /> PDF
            </button>
          </div>
        </div>
      </div>
      <div className="recipe-body">
        <section className="recipe-ingredients">
          <h4>Ingredients</h4>
          {recipe.equipment.length > 0 && <p className="equipment">You'll need: {recipe.equipment.join(", ")}</p>}
          <ul>
            {recipe.ingredients.map((i, k) => (
              <li key={k} className={got.has(k) ? "got" : ""}>
                <label>
                  <input type="checkbox" checked={got.has(k)} onChange={() => setGot(toggle(got, k))} />
                  <span>{ingredientLine(i, recipe.servings, servings)}</span>
                  {i.have && <span className="have">you have</span>}
                </label>
              </li>
            ))}
          </ul>
        </section>
        <section className="recipe-steps">
          <h4>Method</h4>
          <ol>
            {recipe.steps.map((s, k) => (
              <li key={k} className={doneSteps.has(k) ? "done" : ""}>
                <button className="step-num" onClick={() => setDoneSteps(toggle(doneSteps, k))} aria-pressed={doneSteps.has(k)} title="Mark as done">
                  {doneSteps.has(k) ? <Check size={12} /> : k + 1}
                </button>
                <div>
                  <p>{s.text}</p>
                  {s.cue && <p className="cue">Look for: {s.cue}</p>}
                  {s.minutes ? <StepTimer minutes={s.minutes} /> : null}
                </div>
              </li>
            ))}
          </ol>
        </section>
      </div>
      {(recipe.tips.length > 0 || recipe.substitutions.length > 0 || recipe.storage) && (
        <div className="recipe-extra">
          {recipe.tips.length > 0 && (
            <div>
              <h4>Chef's tips</h4>
              <ul>{recipe.tips.map((t, k) => <li key={k}>{t}</li>)}</ul>
            </div>
          )}
          {recipe.substitutions.length > 0 && (
            <div>
              <h4>Swaps</h4>
              <ul>{recipe.substitutions.map((t, k) => <li key={k}>{t}</li>)}</ul>
            </div>
          )}
          {recipe.storage && (
            <div>
              <h4>Storage</h4>
              <p>{recipe.storage}</p>
            </div>
          )}
        </div>
      )}
      {(recipe.sourceUrl || status) && (
        <div className="recipe-foot">
          {recipe.sourceUrl && (
            <button className="link" onClick={() => void openUrl(recipe.sourceUrl)}>
              Based on {recipe.sourceName || "the original recipe"}
            </button>
          )}
          {status && <span className="status">{status}</span>}
        </div>
      )}
    </article>
  );
}
