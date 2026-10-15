import { BookOpen, Search, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { api, errorText } from "../../lib/api";
import { recipeEmoji } from "../../lib/recipe";
import type { SavedRecipe } from "../../lib/types";
import { RecipeCard } from "./RecipeCard";

/** The recipe box: saved recipes, searchable, filtered by category. */
export function RecipeBox({ onClose }: { onClose: () => void }) {
  const [all, setAll] = useState<SavedRecipe[] | null>(null);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string | null>(null);
  const [open, setOpen] = useState<SavedRecipe | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () =>
    api
      .recipesList()
      .then(setAll)
      .catch((e) => setError(errorText(e)));
  useEffect(() => {
    void load();
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && (open ? setOpen(null) : onClose());
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const categories = useMemo(() => [...new Set((all ?? []).map((r) => r.category).filter(Boolean))].sort(), [all]);
  const shown = (all ?? []).filter((r) => {
    if (category && r.category !== category) return false;
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return `${r.title} ${r.category} ${r.recipe.ingredients.map((i) => i.item).join(" ")}`.toLowerCase().includes(q);
  });

  const remove = async (r: SavedRecipe) => {
    try {
      await api.recipeDelete(r.id);
      setOpen(null);
      await load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="recipe-box" role="dialog" aria-modal="true" aria-label="Recipe box">
        <div className="recipe-box-head">
          <BookOpen size={18} />
          <h2>{open ? open.title : "Recipe box"}</h2>
          <span className="spacer" />
          {open && (
            <button className="btn sm ghost" onClick={() => setOpen(null)}>
              All recipes
            </button>
          )}
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {error && <div className="banner danger">{error}</div>}
        {open ? (
          <div className="recipe-box-open">
            <RecipeCard recipe={open.recipe} savedId={open.id} onDelete={() => void remove(open)} />
          </div>
        ) : (
          <>
            <div className="recipe-box-tools">
              <label className="search-field">
                <Search size={14} />
                <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search recipes or ingredients" aria-label="Search recipes" />
              </label>
              <div className="chips">
                <button className="chip" aria-pressed={category == null} onClick={() => setCategory(null)}>
                  All
                </button>
                {categories.map((c) => (
                  <button key={c} className="chip" aria-pressed={category === c} onClick={() => setCategory(category === c ? null : c)}>
                    {c}
                  </button>
                ))}
              </div>
            </div>
            {all == null ? (
              <p className="muted">Loading…</p>
            ) : all.length === 0 ? (
              <div className="empty-box">
                <p>No saved recipes yet.</p>
                <p className="muted">Ask BYTE for a recipe (“how do I make a flat white?”, “what can I make with eggs and spinach?”) and press <b>Save recipe</b>.</p>
              </div>
            ) : (
              <div className="recipe-grid">
                {shown.map((r) => (
                  <button key={r.id} className="recipe-tile" onClick={() => setOpen(r)}>
                    {r.image ? <img src={r.image} alt="" loading="lazy" referrerPolicy="no-referrer" /> : <span className="tile-emoji">{recipeEmoji(r.recipe)}</span>}
                    <span className="tile-title">{r.title}</span>
                    <span className="tile-cat">{r.category}</span>
                  </button>
                ))}
                {shown.length === 0 && <p className="muted">No recipes match.</p>}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
