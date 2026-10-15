import { openUrl } from "@tauri-apps/plugin-opener";
import { Eye, Gamepad2, ShieldQuestion, Star, Store, ThumbsDown, ThumbsUp } from "lucide-react";
import { useState } from "react";

import { checkedText, nextHint, outOfFive, overCheapest, priceText, stockText } from "../../lib/cards";
import type { GameHints, Prices, Reviews, SelfCheck } from "../../lib/types";

const Cites = ({ ns }: { ns: number[] }) => (
  <>
    {ns.map((n) => (
      <span key={n} className="cite-n">
        {n}
      </span>
    ))}
  </>
);

function Stars({ value }: { value: number }) {
  return (
    <span className="stars" aria-label={`${value} out of 5 stars`}>
      {[1, 2, 3, 4, 5].map((i) => (
        <Star key={i} size={12} className={value >= i ? "on" : value >= i - 0.5 ? "half" : ""} />
      ))}
    </span>
  );
}

/** What reviewers and owners say: ratings per site, pros and cons with how many sources say each. */
export function ReviewsCard({ reviews }: { reviews: Reviews }) {
  return (
    <div className="shop-card" role="region" aria-label={`Reviews of ${reviews.product}`}>
      <div className="shop-head">
        <Star size={15} />
        <b>{reviews.product}</b>
        <span className="muted">· {reviews.read} reviews and discussions read</span>
      </div>
      {reviews.verdict && <p className="shop-verdict">{reviews.verdict}</p>}
      {reviews.ratings.length > 0 && (
        <div className="ratings">
          {reviews.ratings.map((r) => (
            <span key={r.n} className="rating">
              <Stars value={outOfFive(r.value, r.best)} />
              <b>
                {r.value}/{r.best}
              </b>
              <span className="muted">
                {r.site}
                {r.count ? ` · ${r.count.toLocaleString("en-US")} ratings` : ""}
              </span>
            </span>
          ))}
        </div>
      )}
      <div className="pros-cons">
        <div>
          <h4>
            <ThumbsUp size={13} /> Pros
          </h4>
          <ul>
            {reviews.pros.map((p) => (
              <li key={p.text}>
                {p.text} <Cites ns={p.sources} />
              </li>
            ))}
          </ul>
        </div>
        <div>
          <h4>
            <ThumbsDown size={13} /> Cons
          </h4>
          <ul>
            {reviews.cons.map((p) => (
              <li key={p.text}>
                {p.text} <Cites ns={p.sources} />
              </li>
            ))}
          </ul>
        </div>
      </div>
      {(reviews.bestFor.length > 0 || reviews.skipIf.length > 0) && (
        <div className="fit">
          {reviews.bestFor.length > 0 && (
            <p>
              <b>Best for:</b> {reviews.bestFor.join(" · ")}
            </p>
          )}
          {reviews.skipIf.length > 0 && (
            <p>
              <b>Skip it if:</b> {reviews.skipIf.join(" · ")}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** Prices read from store pages, cheapest first. */
export function PricesCard({ prices }: { prices: Prices }) {
  const cheapest = prices.offers[0];
  return (
    <div className="shop-card" role="region" aria-label={`Prices for ${prices.product}`}>
      <div className="shop-head">
        <Store size={15} />
        <b>{prices.product}</b>
        <span className="muted">· {checkedText(prices.checkedAt)}; prices change</span>
      </div>
      <table className="offers">
        <tbody>
          {prices.offers.map((o, i) => (
            <tr key={o.store} className={i === 0 ? "best" : ""}>
              <td className="price">{priceText(o.price, o.currency)}</td>
              <td className="diff muted">{cheapest ? overCheapest(o, cheapest) : ""}</td>
              <td>
                <button className="link" onClick={() => void openUrl(o.url)} title={o.title}>
                  {o.store}
                </button>
                <span className="cite-n">{o.n}</span>
              </td>
              <td className="muted">
                {[stockText(o.inStock), o.condition && o.condition !== "new" ? o.condition : ""].filter(Boolean).join(" · ")}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted small">Read from the stores' own pages. Check shipping, tax and memberships before you buy.</p>
    </div>
  );
}

/** Hints that stay hidden until tapped, strongest last, then the full solution. */
export function HintsCard({ hints }: { hints: GameHints }) {
  const [shown, setShown] = useState(0);
  const [solution, setSolution] = useState(false);
  return (
    <div className="shop-card hints" role="region" aria-label="Game hints">
      <div className="shop-head">
        <Gamepad2 size={15} />
        <b>{hints.game || "Hints"}</b>
        {hints.spot && <span className="muted">· {hints.spot}</span>}
      </div>
      <ol className="hint-list">
        {hints.hints.map((h, i) => (
          <li key={i} className={i < shown ? "open" : ""}>
            {i < shown ? h : <span className="muted">Hint {i + 1}{i === 0 ? ": a gentle nudge" : i === hints.hints.length - 1 ? ": nearly the answer" : ""}</span>}
          </li>
        ))}
      </ol>
      <div className="hint-actions">
        {shown < hints.hints.length && (
          <button className="btn sm" onClick={() => setShown(nextHint(shown, hints.hints.length))}>
            <Eye size={13} /> {shown === 0 ? "Show the first hint" : "Next hint"}
          </button>
        )}
        {hints.solution && !solution && (
          <button className="btn sm ghost" onClick={() => setSolution(true)}>
            Show the full solution (spoilers)
          </button>
        )}
        {hints.sources.length > 0 && (
          <span className="muted small">
            From guides <Cites ns={hints.sources} />
          </span>
        )}
      </div>
      {solution && <div className="hint-solution">{hints.solution}</div>}
    </div>
  );
}

/** Claims in the answer that their sources don't clearly back. */
export function SelfCheckNote({ check }: { check: SelfCheck }) {
  const [open, setOpen] = useState(false);
  if (check.issues.length === 0) return null;
  return (
    <div className="self-check">
      <button className="self-check-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <ShieldQuestion size={14} />
        {check.issues.length} of {check.checked} checked claims {check.issues.length === 1 ? "isn't" : "aren't"} clearly backed by their source
      </button>
      {open && (
        <ul>
          {check.issues.map((i) => (
            <li key={i.claim}>
              <span className={`badge ${i.verdict === "no" ? "danger" : "warn"}`}>{i.verdict === "no" ? "Not in source" : "Partly"}</span> {i.claim}{" "}
              <Cites ns={i.sources} />
              {i.note && <div className="muted small">{i.note}</div>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
