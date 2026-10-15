import { CalendarDays, Check, Clock, Copy, ShoppingCart } from "lucide-react";
import { useState } from "react";

import { groceryText, minutesText } from "../../lib/recipe";
import type { MealPlan, RecipeIdeas } from "../../lib/types";
import { useStore } from "../../state/store";

/** "What can I make with…": dishes to pick from; picking one asks for its full recipe. */
export function RecipeIdeasCards({ ideas }: { ideas: RecipeIdeas }) {
  const send = useStore((s) => s.send);
  const generating = useStore((s) => !!s.generating);
  const pick = (title: string) => void send(`Give me the full recipe for ${title}${ideas.have.length ? ` using ${ideas.have.join(", ")}` : ""}.`);
  return (
    <div className="ideas" role="region" aria-label="Dishes you could make">
      {ideas.ideas.map((i) => (
        <button key={i.title} className="idea-card" onClick={() => pick(i.title)} disabled={generating} title="Get the full recipe">
          <span className="idea-emoji" aria-hidden>
            {i.emoji || "🍽️"}
          </span>
          <span className="idea-title">{i.title}</span>
          <span className="idea-desc">{i.description}</span>
          <span className="idea-meta">
            {i.minutes ? (
              <>
                <Clock size={12} /> {minutesText(i.minutes)}
              </>
            ) : null}
            {i.missing.length > 0 ? <span className="missing">Also needs: {i.missing.join(", ")}</span> : <span className="ready">You have everything</span>}
          </span>
        </button>
      ))}
    </div>
  );
}

/** A week of meals and the grocery list; tapping a meal asks for its recipe. */
export function MealPlanCard({ plan }: { plan: MealPlan }) {
  const send = useStore((s) => s.send);
  const generating = useStore((s) => !!s.generating);
  const [copied, setCopied] = useState(false);
  const [bought, setBought] = useState<Set<string>>(new Set());
  const copy = () =>
    void navigator.clipboard?.writeText(groceryText(plan)).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  return (
    <div className="mealplan" role="region" aria-label="Meal plan">
      <div className="mealplan-head">
        <CalendarDays size={15} /> <b>Meal plan</b>
        <span className="hint">Tap a meal for its recipe</span>
      </div>
      <div className="mealplan-days">
        {plan.days.map((d) => (
          <div key={d.day} className="mealplan-day">
            <div className="day-name">{d.day}</div>
            {d.meals.map((m) => (
              <button key={m.meal + m.title} className="plan-meal" onClick={() => void send(`Give me the full recipe for ${m.title}.`)} disabled={generating}>
                <span className="meal-kind">{m.meal}</span>
                <span className="meal-title">
                  <span aria-hidden>{m.emoji || "🍽️"}</span> {m.title}
                </span>
                {m.minutes ? <span className="meal-time">{minutesText(m.minutes)}</span> : null}
              </button>
            ))}
          </div>
        ))}
      </div>
      {plan.grocery.length > 0 && (
        <div className="grocery">
          <div className="grocery-head">
            <ShoppingCart size={14} /> <b>Grocery list</b>
            {plan.have.length > 0 && <span className="hint">Leaves out what you have: {plan.have.join(", ")}</span>}
            <span className="spacer" />
            <button className="btn sm ghost" onClick={copy}>
              {copied ? <Check size={13} /> : <Copy size={13} />} Copy list
            </button>
          </div>
          <div className="grocery-aisles">
            {plan.grocery.map((a) => (
              <div key={a.aisle} className="aisle">
                <div className="aisle-name">{a.aisle}</div>
                <ul>
                  {a.items.map((i) => {
                    const key = `${a.aisle}:${i}`;
                    return (
                      <li key={key}>
                        <label className={bought.has(key) ? "got" : ""}>
                          <input
                            type="checkbox"
                            checked={bought.has(key)}
                            onChange={() => setBought((s) => { const n = new Set(s); if (n.has(key)) n.delete(key); else n.add(key); return n; })}
                          />
                          {i}
                        </label>
                      </li>
                    );
                  })}
                </ul>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
