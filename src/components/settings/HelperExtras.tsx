import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import { errorText } from "../../lib/api";
import { BREAK_MINUTES, FOCUS_MINUTES, READ_SPEEDS, breakLabel, minutesLabel, wakePhraseProblem } from "../../lib/helpers";
import type { Settings } from "../../lib/types";
import { useStore } from "../../state/store";

type Update = (patch: Partial<Settings>) => Promise<unknown>;

/** Focus mode's length and break reminders. */
export function FocusOptions({ settings, update }: { settings: Settings; update: Update }) {
  const minutes = settings.helperFocusMinutes ?? 25;
  const gap = settings.helperFocusBreak ?? 50;
  return (
    <>
      <div className="field">
        <span>Session length<small>“No limit” keeps going until you end it from the tray menu.</small></span>
        <select aria-label="Focus session length" value={minutes} onChange={(e) => void update({ helperFocusMinutes: Number(e.target.value) })}>
          {[...new Set([...FOCUS_MINUTES, minutes])].map((n) => (
            <option key={n} value={n}>{minutesLabel(n)}</option>
          ))}
        </select>
      </div>
      <div className="field">
        <span>Break reminders<small>How often BYTE nudges you to stand up.</small></span>
        <select aria-label="Break reminders" value={gap} onChange={(e) => void update({ helperFocusBreak: Number(e.target.value) })}>
          {[...new Set([...BREAK_MINUTES, gap])].map((n) => (
            <option key={n} value={n}>{breakLabel(n)}</option>
          ))}
        </select>
      </div>
      <label className="field">
        <span>Say it out loud<small>BYTE speaks your plan at the start and a note at the end.</small></span>
        <input type="checkbox" aria-label="Say the focus plan out loud" checked={settings.helperFocusSpeak ?? true} onChange={(e) => void update({ helperFocusSpeak: e.target.checked })} />
      </label>
    </>
  );
}

/** Home Assistant's address and token. The token goes straight to the system credential store and is never shown again. */
export function HomeAssistantOptions({ settings, update }: { settings: Settings; update: Update }) {
  const [url, setUrl] = useState(settings.helperHaUrl ?? "");
  const [token, setToken] = useState("");
  const [saved, setSaved] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    void invoke<boolean>("ha_token_saved").then(setSaved, () => undefined);
  }, []);
  const run = (p: Promise<unknown>, ok: string) => p.then(() => setNote(ok), (e) => setNote(errorText(e)));
  return (
    <div className="field" style={{ display: "block" }}>
      <span>Home Assistant address<small>On your home network, like http://homeassistant.local:8123. Make a long-lived access token in your Home Assistant profile.</small></span>
      <input aria-label="Home Assistant address" style={{ width: "100%", marginTop: 6 }} value={url} placeholder="http://homeassistant.local:8123" onChange={(e) => setUrl(e.target.value)} onBlur={() => void update({ helperHaUrl: url.trim() })} />
      <div className="row" style={{ gap: 8, marginTop: 6, flexWrap: "wrap" }}>
        <input aria-label="Home Assistant access token" type="password" autoComplete="off" style={{ flex: 1, minWidth: 180 }} value={token} placeholder={saved ? "Token saved (type to replace)" : "Paste the access token"} onChange={(e) => setToken(e.target.value)} />
        <button className="btn sm" disabled={!token} onClick={() => void run(invoke("ha_set_token", { token }).then(() => { setToken(""); setSaved(true); }), "Token saved in the credential store.")}>Save token</button>
        {saved && <button className="btn sm" onClick={() => void run(invoke("ha_clear_token").then(() => setSaved(false)), "Token removed.")}>Remove token</button>}
        <button className="btn sm" onClick={() => void update({ helperHaUrl: url.trim() }).then(() => invoke<string>("ha_test")).then(setNote, (e) => setNote(errorText(e)))}>Test</button>
      </div>
      {note && <small role="status">{note}</small>}
    </div>
  );
}

interface ReviewStep {
  text: string;
  level: "ok" | "look";
  note: string;
}
interface Review {
  phrase: string;
  steps: ReviewStep[];
  problems: string[];
  canImport: boolean;
}
interface Routine {
  id: string;
  phrase: string;
}

/** Recipes: export a routine as a file, or read someone else's recipe, review it and add it. */
export function RecipeOptions() {
  const [routines, setRoutines] = useState<Routine[]>([]);
  const [text, setText] = useState("");
  const [review, setReview] = useState<Review | null>(null);
  const [phrase, setPhrase] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const settings = useStore((s) => s.settings);
  useEffect(() => {
    if (settings?.helperRecipes) void invoke<Routine[]>("routines_list").then(setRoutines, () => undefined);
  }, [settings?.helperRecipes, note]);
  const fail = (e: unknown) => setNote(errorText(e));
  return (
    <div className="field" style={{ display: "block" }}>
      <span>Share a routine<small>Saves it in Documents/BYTE/Recipes as a small file you can send. It never includes a schedule.</small></span>
      <div className="row" style={{ gap: 6, flexWrap: "wrap", marginTop: 6 }}>
        {routines.length === 0 && <small>You have no routines yet.</small>}
        {routines.map((r) => (
          <button key={r.id} className="btn sm" onClick={() => void invoke<string>("recipe_export", { id: r.id }).then((p) => setNote(`Saved: ${p}`), fail)}>Export “{r.phrase}”</button>
        ))}
      </div>
      <span style={{ display: "block", marginTop: 10 }}>Add a recipe<small>Paste the recipe file's text. BYTE shows every step first.</small></span>
      <textarea aria-label="Recipe text" rows={5} style={{ width: "100%", marginTop: 6 }} value={text} onChange={(e) => { setText(e.target.value); setReview(null); }} />
      <button className="btn sm" style={{ marginTop: 6 }} disabled={!text.trim()} onClick={() => void invoke<Review>("recipe_review", { text }).then((r) => { setReview(r); setPhrase(r.phrase); setNote(null); }, fail)}>Review it</button>
      {review && (
        <div role="region" aria-label="Recipe review" style={{ marginTop: 8 }}>
          <strong>When you say “{review.phrase}” BYTE will:</strong>
          <ol>
            {review.steps.map((s, i) => (
              <li key={i}>{s.text}{s.level === "look" && <small> — look twice: {s.note}</small>}</li>
            ))}
          </ol>
          {review.problems.map((p) => <p key={p} className="error">{p}</p>)}
          {review.canImport || review.problems.length > 0 ? (
            <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
              <input aria-label="Phrase to say" value={phrase} onChange={(e) => setPhrase(e.target.value)} />
              <button className="btn sm" disabled={!phrase.trim()} onClick={() => void invoke("recipe_import", { text, phrase }).then(() => { setNote("Added to your routines."); setReview(null); setText(""); }, fail)}>Add it</button>
            </div>
          ) : null}
        </div>
      )}
      {note && <small role="status" style={{ display: "block", marginTop: 6 }}>{note}</small>}
    </div>
  );
}

/** Reading speed for read aloud. */
export function ReadOptions({ settings, update }: { settings: Settings; update: Update }) {
  const speed = settings.helperReadSpeed ?? "normal";
  return (
    <div className="field">
      <span>Reading speed</span>
      <select aria-label="Reading speed" value={speed} onChange={(e) => void update({ helperReadSpeed: e.target.value as "slow" | "normal" | "fast" })}>
        {READ_SPEEDS.map((o) => (
          <option key={o.id} value={o.id}>{o.label}</option>
        ))}
      </select>
    </div>
  );
}

/** A wake phrase of your own. */
export function WakePhrase({ settings, update }: { settings: Settings; update: Update }) {
  const [text, setText] = useState(settings.helperWakePhrase ?? "");
  const [error, setError] = useState<string | null>(null);
  const save = () => {
    const problem = wakePhraseProblem(text);
    setError(problem);
    if (!problem) void update({ helperWakePhrase: text.trim() });
  };
  return (
    <div className="field" style={{ display: "block" }}>
      <span>
        Your own wake phrase
        <small>
          Say this instead of “Hey BYTE” (one to three words, 6 letters or more, like “hey jarvis”). Leave it empty for “Hey BYTE”. BYTE finds it in what the speech model heard, so
          a longer, unusual phrase works better than a short common word, and it is less sure than “Hey BYTE”: expect some misses and the odd false trigger.
        </small>
      </span>
      <input aria-label="Your own wake phrase" style={{ width: "100%", marginTop: 6 }} value={text} placeholder="Hey BYTE" onChange={(e) => setText(e.target.value)} onBlur={save} onKeyDown={(e) => e.key === "Enter" && save()} />
      {error && <small className="error" role="alert">{error}</small>}
    </div>
  );
}

/** The details the form helper may type. There is no place here for a password or a card number, and BYTE refuses one if you try. */
export function FormProfile() {
  const [fields, setFields] = useState<[string, string][]>([]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    void invoke<[Record<string, string>, [string, string][]]>("form_profile_get").then(([p, f]) => { setValues(p); setFields(f); }, () => undefined);
  }, []);
  const save = () =>
    invoke<string[]>("form_profile_set", { profile: values }).then(
      (refused) => setNote(refused.length ? `Not saved (it looks like a card number or password): ${refused.join(", ")}` : "Saved on this computer."),
      (e) => setNote(errorText(e)),
    );
  return (
    <div className="field" style={{ display: "block" }}>
      <span>Your details<small>Kept on this computer only. Never put a password or card number here.</small></span>
      <div style={{ display: "grid", gridTemplateColumns: "minmax(110px, 160px) 1fr", gap: 6, marginTop: 6 }}>
        {fields.map(([key, label]) => (
          <label key={key} style={{ display: "contents" }}>
            <span>{label}</span>
            <input aria-label={label} value={values[key] ?? ""} onChange={(e) => setValues({ ...values, [key]: e.target.value })} />
          </label>
        ))}
      </div>
      <button className="btn sm" style={{ marginTop: 6 }} onClick={() => void save()}>Save details</button>
      {note && <small role="status" style={{ display: "block" }}>{note}</small>}
    </div>
  );
}
