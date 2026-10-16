import type { FirstRunFeature, FirstRunId } from "../../lib/firstRun";

/** First-run question: each feature that is off until chosen, with one sentence on what it does. Nothing is ticked at the start. */
export function FirstRunFeatures({
  features,
  ticked,
  onChange,
  controlModel,
}: {
  features: FirstRunFeature[];
  ticked: FirstRunId[];
  onChange: (next: FirstRunId[]) => void;
  /** Name of the PC-control model being downloaded, if any. */
  controlModel: string | null;
}) {
  const toggle = (id: FirstRunId) => onChange(ticked.includes(id) ? ticked.filter((x) => x !== id) : [...ticked, id]);
  if (features.length === 0) {
    return (
      <>
        <h1>Optional features</h1>
        <p className="lead">There is nothing to switch on here. You can look at every option later in Settings.</p>
      </>
    );
  }
  return (
    <>
      <h1>Optional features</h1>
      <p className="lead">
        These are all off. Tick only the ones you want now; each can be turned on or off any time in Settings, and none of them runs until you say so.
      </p>
      <div role="group" aria-label="Optional features" className="first-run-list">
        {features.map((f) => (
          <label key={f.id} className="field">
            <span>
              {f.title}
              <small>{f.explain}</small>
              {f.id === "pcControl" && controlModel && ticked.includes("pcControl") && <small>It will use {controlModel} to plan.</small>}
            </span>
            <input type="checkbox" checked={ticked.includes(f.id)} onChange={() => toggle(f.id)} aria-label={`Turn on ${f.title}`} />
          </label>
        ))}
      </div>
    </>
  );
}
