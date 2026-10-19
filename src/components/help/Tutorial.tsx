import { ArrowLeft, ArrowRight, AudioLines, Check, CheckCircle2, Clock, Compass, FolderSearch, Gamepad2, Globe, Hand, LifeBuoy, MessageSquare, Mic, Monitor, MousePointer2, Image as ImageIcon, Paperclip, Rocket, Save, Search, Shield, X, XCircle, Zap } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ComponentType } from "react";

import { api, inTauri } from "../../lib/api";
import { currentOs } from "../../lib/platform";
import {
  loadProgress,
  nextInRun,
  recommended,
  relatedNext,
  resumePoint,
  runPosition,
  saveProgress,
  searchTutorials,
  statusOf,
  stepsFor,
  tutorialsFor,
  withDone,
  withStep,
  type CheckId,
  type LinkKind,
  type Progress,
  type Step,
  type Store,
  type Tutorial as TutorialData,
} from "../../lib/tutorial";
import { useStore, type SettingsTab } from "../../state/store";

const ICONS: Record<string, ComponentType<{ size?: number }>> = {
  rocket: Rocket, chat: MessageSquare, paperclip: Paperclip, globe: Globe, mic: Mic, wave: AudioLines, hand: Hand, bolt: Zap, clock: Clock,
  image: ImageIcon, pointer: MousePointer2, folder: FolderSearch, screen: Monitor, compass: Compass, game: Gamepad2, save: Save, shield: Shield, lifebuoy: LifeBuoy,
};

const BADGE = { done: "Done", progress: "In progress", new: "Not started" } as const;

function storage(): Store | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The answer to a live check, or null while it is being asked. */
function useChecks(): (id: CheckId) => boolean | null {
  const settings = useStore((s) => s.settings) as unknown as Record<string, unknown> | null;
  const [speech, setSpeech] = useState<boolean | null>(null);
  useEffect(() => {
    if (!inTauri) {
      setSpeech(false);
      return;
    }
    void api.wakeReady().then(setSpeech, () => setSpeech(false));
  }, []);
  return (id) => {
    const on = (k: string) => !!settings?.[k];
    switch (id) {
      case "speechModel": return speech;
      case "heyByteOn": return on("wakeWord");
      case "pcControlOn": return on("pcControl");
      case "screenAskOn": return on("screenAsk");
      case "replayOn": return on("replayBuffer");
      case "browserOn": return on("browserVisible") || on("browserLiveResearch") || on("browserDriveReal");
      case "routinesOn": return on("routinesEnabled");
      case "findFilesOn": return on("findFiles");
      case "cloudConnected": return on("cloudConnected");
    }
  };
}

/** The tutorial: a home screen with one card per feature, and a short walkthrough for each. */
/** `autoStart`: begin the whole run at once (the first-run "show me around" button). Esc or Close leaves it any time. */
export function Tutorial({ onClose, autoStart = false }: { onClose: () => void; autoStart?: boolean }) {
  const os = currentOs();
  const list = useMemo(() => tutorialsFor(os), [os]);
  const openSettings = useStore((s) => s.openSettings);
  const check = useChecks();
  const [progress, setProgress] = useState<Progress>(() => loadProgress(storage()));
  const [openId, setOpenId] = useState<string | null>(null);
  const [step, setStep] = useState(0);
  const [query, setQuery] = useState("");
  const panel = useRef<HTMLDivElement>(null);

  const commit = (p: Progress) => {
    setProgress(p);
    saveProgress(storage(), p);
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  useEffect(() => {
    panel.current?.focus();
  }, [openId, step]);

  const tutorial = openId ? list.find((t) => t.id === openId) ?? null : null;
  const run = progress.all.active;

  const open = (id: string, from?: number) => {
    const t = list.find((x) => x.id === id);
    if (!t) return;
    setOpenId(id);
    setStep(Math.min(from ?? progress.step[id] ?? 0, stepsFor(t, os).length));
  };
  const home = () => {
    setOpenId(null);
    setQuery("");
  };
  const startAll = () => {
    const fresh: Progress = { ...progress, all: { active: true, skipped: [] } };
    commit(fresh);
    const first = resumePoint(list, fresh);
    if (first) open(first, 0);
  };
  useEffect(() => {
    if (autoStart) startAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const stopRun = () => commit({ ...progress, all: { active: false, skipped: progress.all.skipped } });
  const goStep = (t: TutorialData, n: number) => {
    setStep(n);
    const total = stepsFor(t, os).length;
    commit(n >= total ? withDone(progress, t.id) : withStep(progress, t.id, n));
  };
  const skipThis = (t: TutorialData) => {
    const skipped = [...progress.all.skipped, t.id];
    const next = nextInRun(list, skipped, t.id);
    commit({ ...progress, all: { active: true, skipped } });
    return next ? open(next, 0) : home();
  };
  const act = (l: LinkKind) => {
    if (l.kind === "setting") {
      onClose();
      openSettings(l.tab as SettingsTab);
    } else if (l.kind === "try") {
      useStore.setState({ prefill: l.text });
      onClose();
    }
  };

  const rec = recommended(list, progress);
  const shown = searchTutorials(list, query);
  const doneCount = list.filter((t) => progress.done.includes(t.id)).length;

  return (
    <div className="scrim tutorial-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="tutorial" role="dialog" aria-modal="true" aria-label="Tutorial" ref={panel} tabIndex={-1}>
        <div className="tutorial-head">
          <h2>{tutorial ? tutorial.title : "Tutorial"}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close the tutorial (Esc). Your place is saved.">
            <X size={16} />
          </button>
        </div>
        {!tutorial && (
          <Home
            list={shown}
            all={list}
            progress={progress}
            rec={query ? null : rec}
            query={query}
            setQuery={setQuery}
            onOpen={(id) => open(id)}
            onAll={startAll}
            resumeRun={run && resumePoint(list, progress) !== null}
            doneCount={doneCount}
          />
        )}
        {tutorial && (
          <Walk
            t={tutorial}
            os={os}
            step={step}
            list={list}
            progress={progress}
            check={check}
            onStep={(n) => goStep(tutorial, n)}
            onHome={home}
            onAct={act}
            onSkip={() => skipThis(tutorial)}
            onOpen={(id) => open(id, 0)}
            onStopRun={stopRun}
          />
        )}
      </div>
    </div>
  );
}

function Home(p: {
  list: TutorialData[];
  all: TutorialData[];
  progress: Progress;
  rec: TutorialData | null;
  query: string;
  setQuery: (q: string) => void;
  onOpen: (id: string) => void;
  onAll: () => void;
  resumeRun: boolean;
  doneCount: number;
}) {
  return (
    <div className="tutorial-body">
      <p className="tutorial-welcome">
        Welcome! Pick the thing you want to learn, or do them all in order. Each one is short, and BYTE remembers where you stopped. Nothing here changes your settings unless you click.
      </p>
      <button className="btn primary tutorial-all" onClick={p.onAll} aria-label={p.resumeRun ? "Continue the whole tutorial" : "Do the whole tutorial"}>
        {p.resumeRun ? "Continue the whole tutorial" : "Do the whole tutorial"} <ArrowRight size={16} />
      </button>
      <p className="faint small">
        {p.doneCount} of {p.all.length} done
      </p>
      {p.rec && (
        <div className="tutorial-rec">
          <span className="faint small">Recommended for you</span>
          <button className="tutorial-card on" onClick={() => p.onOpen(p.rec!.id)}>
            <b>{p.rec.title}</b>
            <span>{p.rec.promise}</span>
          </button>
        </div>
      )}
      <label className="search">
        <Search size={14} className="faint" />
        <input value={p.query} onChange={(e) => p.setQuery(e.target.value)} placeholder="Search the tutorials" aria-label="Search the tutorials" />
      </label>
      {p.list.length === 0 && <p className="faint small">Nothing matches. Try other words.</p>}
      <ul className="tutorial-grid" aria-label="Tutorials">
        {p.list.map((t) => {
          const Icon = ICONS[t.icon] ?? Rocket;
          const st = statusOf(p.progress, t.id);
          return (
            <li key={t.id}>
              <button className="tutorial-card" onClick={() => p.onOpen(t.id)} aria-label={`${t.title}. ${t.promise}. ${t.minutes} minutes. ${BADGE[st]}.${t.offByDefault ? " Off by default." : ""}`}>
                <span className="tutorial-icon">
                  <Icon size={18} />
                </span>
                <b>{t.title}</b>
                <span>{t.promise}</span>
                <span className="tutorial-meta">
                  <span>{t.minutes} min</span>
                  <span className={`tutorial-badge ${st}`}>{BADGE[st]}</span>
                  {t.offByDefault && <span className="tutorial-tag">Off by default</span>}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function Walk(p: {
  t: TutorialData;
  os: ReturnType<typeof currentOs>;
  step: number;
  list: TutorialData[];
  progress: Progress;
  check: (id: CheckId) => boolean | null;
  onStep: (n: number) => void;
  onHome: () => void;
  onAct: (l: LinkKind) => void;
  onSkip: () => void;
  onOpen: (id: string) => void;
  onStopRun: () => void;
}) {
  const steps = stepsFor(p.t, p.os);
  const total = steps.length;
  const finished = p.step >= total;
  const s: Step | undefined = steps[p.step];
  const run = p.progress.all.active;
  const pos = runPosition(p.list, p.progress.all.skipped, p.t.id);
  const next = run ? nextInRun(p.list, p.progress.all.skipped, p.t.id) : null;
  const related = relatedNext(p.list, p.progress, p.t.id);
  const nextTut = (id: string | null) => p.list.find((x) => x.id === id) ?? null;
  const offer = next ? nextTut(next) : related;
  const pct = Math.round((Math.min(p.step, total) / total) * 100);
  return (
    <div className="tutorial-body">
      {run && (
        <p className="faint small" aria-live="polite">
          Tutorial {pos.n} of {pos.of}
        </p>
      )}
      <div className="tutorial-progress" role="progressbar" aria-valuemin={0} aria-valuemax={total} aria-valuenow={Math.min(p.step, total)} aria-label="Progress in this tutorial">
        <span style={{ width: `${pct}%` }} />
      </div>
      {!finished && s && (
        <>
          <p className="faint small">
            Step {p.step + 1} of {total}
          </p>
          {p.step === 0 && (
            <p className="tutorial-promise">
              <b>What you'll be able to do:</b> {p.t.promise}.
            </p>
          )}
          <h3>{s.title}</h3>
          <div className="tutorial-text">
            {s.body.map((b, i) =>
              b.startsWith("- ") ? (
                <p key={i} className="tutorial-bullet">
                  {b.slice(2)}
                </p>
              ) : (
                <p key={i}>{b}</p>
              ),
            )}
          </div>
          {s.example && (
            <p className="tutorial-example">
              <span className="faint small">Say or type</span>
              <code>{s.example}</code>
            </p>
          )}
          {s.check && <LiveCheck label={s.check.label} value={p.check(s.check.id)} />}
          {s.links && (
            <div className="tutorial-actions">
              {s.links.map((l, i) => (
                <button key={i} className="btn" onClick={() => p.onAct(l)}>
                  {l.kind === "help" ? "Read: " : ""}
                  {l.label}
                </button>
              ))}
            </div>
          )}
          {s.ifStuck && (
            <p className="tutorial-stuck">
              <b>If it doesn't work:</b> {s.ifStuck}
            </p>
          )}
        </>
      )}
      {finished && (
        <div>
          <h3>
            <CheckCircle2 size={18} /> You're done with {p.t.title}
          </h3>
          <p>Nice work. You can come back to this any time from Help, then Tutorial.</p>
          {offer && (
            <button className="btn primary" onClick={() => p.onOpen(offer.id)}>
              {run ? "Next: " : "Up next: "}
              {offer.title} <ArrowRight size={14} />
            </button>
          )}
          {!offer && <p>That was the last one. You've seen everything.</p>}
        </div>
      )}
      <div className="tutorial-nav">
        <button className="btn" onClick={() => p.onStep(Math.max(0, p.step - 1))} disabled={p.step === 0}>
          <ArrowLeft size={14} /> Back
        </button>
        {!finished && (
          <button className="btn primary" onClick={() => p.onStep(p.step + 1)}>
            {p.step === total - 1 ? "Finish" : "Next"} <ArrowRight size={14} />
          </button>
        )}
        <span className="grow" />
        {run && !finished && (
          <button className="btn ghost" onClick={p.onSkip}>
            Skip this one
          </button>
        )}
        {run && (
          <button className="btn ghost" onClick={p.onStopRun}>
            Pause the whole tutorial
          </button>
        )}
        <button className="btn ghost" onClick={p.onHome}>
          Back to all tutorials
        </button>
      </div>
    </div>
  );
}

function LiveCheck({ label, value }: { label: string; value: boolean | null }) {
  return (
    <p className="tutorial-check" role="status">
      {value === null ? <span className="faint">Checking…</span> : value ? <Check size={14} className="ok" /> : <XCircle size={14} className="bad" />}{" "}
      {label}: <b>{value === null ? "…" : value ? "yes" : "no"}</b>
    </p>
  );
}
