import { Check, Copy, RotateCcw, Trophy } from "lucide-react";
import { useMemo, useState } from "react";

import { decisionMarkdown, ranking, weightedTotals } from "../../lib/decide";
import type { Decision, Source } from "../../lib/types";

/**
 * Compare & decide score table: options side by side, a weight slider per
 * criterion, and totals that re-rank as the weights change. Hovering a score
 * shows its reason and sources.
 */
export function DecisionTable({ decision, sources }: { decision: Decision; sources?: Source[] }) {
  const initial = useMemo(() => decision.criteria.map((c) => c.weight), [decision]);
  const [weights, setWeights] = useState(initial);
  const [copied, setCopied] = useState(false);
  const totals = weightedTotals(decision, weights);
  const order = ranking(totals);
  const best = order[0];
  const changed = weights.some((w, i) => w !== initial[i]);
  const title = (n: number) => sources?.find((s) => s.n === n)?.title ?? `Source ${n}`;

  const copy = () => {
    void navigator.clipboard?.writeText(decisionMarkdown(decision, weights)).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div className="decision" role="region" aria-label="Comparison">
      <div className="decision-head">
        <span className="decision-title">Comparison</span>
        <span className="hint">Drag the weights to match what matters to you</span>
        <span className="spacer" />
        {changed && (
          <button className="btn sm ghost" onClick={() => setWeights(initial)}>
            <RotateCcw size={13} /> Reset
          </button>
        )}
        <button className="btn sm ghost" onClick={copy}>
          {copied ? <Check size={13} /> : <Copy size={13} />} Copy
        </button>
      </div>
      <div className="decision-scroll">
        <table>
          <thead>
            <tr>
              <th className="crit">What matters</th>
              {decision.options.map((o, i) => (
                <th key={o} className={i === best ? "best" : undefined}>
                  {i === best && <Trophy size={13} aria-label="Best pick" />} {o}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {decision.criteria.map((c, j) => (
              <tr key={c.name}>
                <td className="crit">
                  <span className="crit-name">{c.name}</span>
                  <input
                    type="range"
                    min={0}
                    max={5}
                    step={1}
                    value={weights[j]}
                    aria-label={`How much ${c.name} matters`}
                    onChange={(e) => setWeights(weights.map((w, k) => (k === j ? Number(e.target.value) : w)))}
                  />
                  <span className="weight">{weights[j]}</span>
                </td>
                {decision.scores.map((row, i) => {
                  const cell = row[j];
                  const tip = cell ? [cell.reason, ...cell.sources.map((n) => `[${n}] ${title(n)}`)].filter(Boolean).join("\n") : "Not scored";
                  return (
                    <td key={i} title={tip} className={weights[j] === 0 ? "muted" : undefined}>
                      {cell ? (
                        <span className="score">
                          <span className="bar" style={{ width: `${cell.score * 10}%` }} />
                          <b>{cell.score}</b>
                          {cell.sources.length > 0 && <sup className="cite-n">{cell.sources.join(",")}</sup>}
                        </span>
                      ) : (
                        <span className="none">–</span>
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <td className="crit">Weighted score</td>
              {totals.map((t, i) => (
                <td key={i} className={i === best ? "best" : undefined}>
                  <b>{t.toFixed(1)}</b>
                  <span className="rank">#{order.indexOf(i) + 1}</span>
                </td>
              ))}
            </tr>
          </tfoot>
        </table>
      </div>
    </div>
  );
}
