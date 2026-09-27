import { AnimatePresence, motion } from "framer-motion";
import {
  ArrowUp,
  Brain,
  CircleCheck,
  Cpu,
  Gauge,
  HardDrive,
  Keyboard,
  MemoryStick,
  Rocket,
  ShieldCheck,
  Sparkles,
  Telescope,
  TriangleAlert,
  Zap,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { Logo } from "../../design/Logo";
import { api, errorText } from "../../lib/api";
import { bytes, contextLabel, ramSize } from "../../lib/format";
import type { ModelStatus } from "../../lib/types";
import { useStore } from "../../state/store";
import { DownloadProgress, FitPill } from "../models/ModelCard";

const STEPS = 5;

/** Picks the best chat model that runs well on this Mac. */
export function pickDefault(models: ModelStatus[]): ModelStatus | undefined {
  const chat = models.filter((m) => m.role === "chat" && m.fit.fit !== "toobig");
  return (
    chat.find((m) => m.installed && m.recommended) ??
    chat.find((m) => m.recommended && m.fit.fit === "great") ??
    chat.find((m) => m.fit.fit === "great") ??
    chat[0]
  );
}

export function Onboarding() {
  const [step, setStep] = useState(0);
  const models = useStore((s) => s.models);
  const system = useStore((s) => s.system);
  const downloads = useStore((s) => s.downloads);
  const refresh = useStore((s) => s.refreshModels);
  const update = useStore((s) => s.updateSettings);
  const [choice, setChoice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!choice && models.length) setChoice(pickDefault(models)?.id ?? null);
  }, [models, choice]);

  const chosen = models.find((m) => m.id === choice);
  const dl = choice ? downloads[choice] : undefined;
  const installed = !!chosen?.installed || dl?.phase === "finished";

  // Move on automatically once the download completes.
  useEffect(() => {
    if (step === 3 && installed) {
      const t = setTimeout(() => setStep(4), 900);
      return () => clearTimeout(t);
    }
  }, [step, installed]);

  const startDownload = async () => {
    if (!choice) return;
    setError(null);
    try {
      if (!chosen?.installed) await api.modelDownload(choice);
      setStep(3);
    } catch (e) {
      setError(errorText(e));
    }
    void refresh();
  };

  const finish = async () => {
    setError(null);
    try {
      await update({ onboardingComplete: true, activeModel: choice });
      if (choice) void api.modelActivate(choice).catch(() => {});
    } catch (e) {
      setError(errorText(e));
    }
  };

  const macNote = useMemo(() => {
    if (!system) return null;
    const gb = system.totalRamBytes / 2 ** 30;
    if (!system.appleSilicon) return { ok: false, text: "BYTE is built for Apple Silicon Macs (M1 or newer). It may not run well here." };
    if (gb < 12) return { ok: false, text: "With 8 GB of memory, BYTE will use the smaller Fast model." };
    if (gb < 20) return { ok: true, text: "Your Mac can run BYTE's Smart model comfortably." };
    return { ok: true, text: "Your Mac has plenty of memory — every BYTE model will run well." };
  }, [system]);

  return (
    <div className="onboarding">
      <div className="titlebar" data-tauri-drag-region />
      <div className="onb-stage">
        <AnimatePresence mode="wait">
          <motion.div
            key={step}
            className="onb-card"
            initial={{ opacity: 0, y: 14 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            transition={{ duration: 0.22, ease: [0.2, 0.8, 0.2, 1] }}
          >
            {step === 0 && (
              <>
                <Logo size={84} />
                <h1>Meet BYTE.</h1>
                <p className="lead">A powerful AI assistant that lives entirely on your Mac.</p>
                <ul className="tips">
                  <li>
                    <ShieldCheck size={18} />
                    <div><b>Private by design.</b> <span className="muted">Your chats and files never leave this Mac. No account, no subscription.</span></div>
                  </li>
                  <li>
                    <Brain size={18} />
                    <div><b>Thinks things through.</b> <span className="muted">Turn on thinking for hard questions and watch it reason step by step.</span></div>
                  </li>
                  <li>
                    <Cpu size={18} />
                    <div><b>Uses your Mac's hardware.</b> <span className="muted">Runs on the Apple Silicon GPU, even offline.</span></div>
                  </li>
                </ul>
                <div className="onb-actions">
                  <span />
                  <button className="btn lg primary" onClick={() => setStep(1)}>Get started</button>
                </div>
              </>
            )}

            {step === 1 && system && (
              <>
                <h1>Checking your Mac</h1>
                <p className="lead">BYTE picks the best model for your hardware.</p>
                <div className="spec-grid">
                  <div className="spec">
                    <div className="k"><Cpu size={14} /> Chip</div>
                    <div className="v">{system.chip.replace("Apple ", "")}</div>
                  </div>
                  <div className="spec">
                    <div className="k"><MemoryStick size={14} /> Memory</div>
                    <div className="v">{ramSize(system.totalRamBytes)}</div>
                  </div>
                  <div className="spec">
                    <div className="k"><HardDrive size={14} /> Free disk</div>
                    <div className="v">{bytes(system.freeDiskBytes, 0)}</div>
                  </div>
                </div>
                {macNote && (
                  <div className={`banner ${macNote.ok ? "" : "danger"}`} style={{ marginTop: 16 }}>
                    {macNote.ok ? <CircleCheck size={18} style={{ color: "var(--ok)" }} /> : <TriangleAlert size={18} style={{ color: "var(--warn)" }} />}
                    <span className="grow">{macNote.text}</span>
                  </div>
                )}
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(0)}>Back</button>
                  <button className="btn lg primary" onClick={() => setStep(2)}>Continue</button>
                </div>
              </>
            )}

            {step === 2 && (
              <>
                <h1>Choose your model</h1>
                <p className="lead">This is BYTE's brain. You can download others later in Settings.</p>
                <div className="model-list">
                  {models
                    .filter((m) => m.role === "chat")
                    .map((m) => {
                      const disabled = m.fit.fit === "toobig";
                      return (
                        <button
                          key={m.id}
                          className={`model-card selectable ${disabled ? "disabled" : ""}`}
                          aria-pressed={choice === m.id}
                          disabled={disabled}
                          onClick={() => setChoice(m.id)}
                        >
                          <div className="title">
                            {m.name}
                            {m.recommended && <span className="pill accent">Recommended</span>}
                            {m.installed && <span className="pill ok">Downloaded</span>}
                          </div>
                          <div className="muted" style={{ fontSize: "0.92em" }}>{m.tagline}</div>
                          <div className="meta">
                            <span>{bytes(m.sizeBytes)} download</span>
                            <span>· {m.speedHint} on M4</span>
                            <span style={{ marginLeft: "auto" }}><FitPill model={m} /></span>
                          </div>
                          {disabled && <div className="faint" style={{ fontSize: "0.85em" }}>{m.fit.note}</div>}
                        </button>
                      );
                    })}
                </div>
                {system && chosen && !chosen.installed && system.freeDiskBytes > 0 && system.freeDiskBytes < chosen.sizeBytes + 1e9 && (
                  <div className="banner danger" style={{ marginTop: 12 }}>
                    <TriangleAlert size={18} />
                    <span className="grow">Not enough free disk space for this model. Free up {bytes(chosen.sizeBytes + 1e9 - system.freeDiskBytes)} and try again.</span>
                  </div>
                )}
                {error && <div className="banner danger" style={{ marginTop: 12 }}>{error}</div>}
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(1)}>Back</button>
                  <button className="btn lg primary" onClick={startDownload} disabled={!chosen}>
                    {chosen?.installed ? "Continue" : `Download ${chosen ? bytes(chosen.sizeBytes) : ""}`}
                  </button>
                </div>
              </>
            )}

            {step === 3 && chosen && (
              <>
                <h1>{installed ? "All set." : `Downloading ${chosen.name}`}</h1>
                <p className="lead">
                  {installed
                    ? "The model is verified and ready."
                    : "This is a one-time download. You can pause and resume any time — even after quitting."}
                </p>
                <div className="model-card">
                  <div className="title">{chosen.name}</div>
                  <DownloadProgress model={chosen} dl={dl ?? (installed ? { phase: "finished", bytes: chosen.sizeBytes, total: chosen.sizeBytes, bytesPerSec: 0 } : undefined)} />
                </div>
                {dl?.phase === "failed" && (
                  <div className="banner danger" style={{ marginTop: 12 }}>
                    <span className="grow">{dl.error}</span>
                    <button className="btn sm" onClick={startDownload}>Retry</button>
                  </div>
                )}
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(2)}>Back</button>
                  <div className="row">
                    {!installed && dl?.phase === "paused" && (
                      <button className="btn" onClick={startDownload}>Resume</button>
                    )}
                    {!installed && dl?.phase !== "paused" && dl?.phase !== "failed" && (
                      <button className="btn" onClick={() => void api.modelPause(chosen.id)}>Pause</button>
                    )}
                    <button className="btn lg primary" onClick={() => setStep(4)} disabled={!installed}>
                      Continue
                    </button>
                  </div>
                </div>
              </>
            )}

            {step === 4 && (
              <>
                <h1>A few quick tips</h1>
                <p className="lead">You'll pick these up in no time.</p>
                <ul className="tips">
                  <li>
                    <Gauge size={18} />
                    <div>
                      <b>Four modes.</b>{" "}
                      <span className="muted">
                        <Zap size={12} /> Fast for quick answers, <Gauge size={12} /> Auto decides for you, <Telescope size={12} /> Deep for thorough answers,{" "}
                        <Rocket size={12} /> Extended for the most complete work.
                      </span>
                    </div>
                  </li>
                  <li>
                    <Brain size={18} />
                    <div><b>Thinking.</b> <span className="muted">Click the thinking button to force it on or off. Open “Thought for…” to see how BYTE reasoned.</span></div>
                  </li>
                  <li>
                    <Keyboard size={18} />
                    <div>
                      <b>Shortcuts.</b> <span className="muted"><kbd>⌘N</kbd> new chat · <kbd>⌘,</kbd> settings · <kbd>⌘\</kbd> sidebar · <kbd>Esc</kbd> stop</span>
                    </div>
                  </li>
                  <li>
                    <Sparkles size={18} />
                    <div><b>More is coming.</b> <span className="muted">Web search, documents, file reading and Mac controls arrive in upcoming test builds — BYTE updates itself.</span></div>
                  </li>
                </ul>
                {chosen && (
                  <p className="faint" style={{ fontSize: "0.88em", marginTop: 16 }}>
                    {chosen.name} will use about {bytes(chosen.fit.neededBytes)} of memory with a {contextLabel(chosen.fit.context)}-token context.
                  </p>
                )}
                {error && <div className="banner danger">{error}</div>}
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(3)}>Back</button>
                  <button className="btn lg primary" onClick={finish}>
                    Start chatting <ArrowUp size={16} style={{ transform: "rotate(90deg)" }} />
                  </button>
                </div>
              </>
            )}
          </motion.div>
        </AnimatePresence>
      </div>
      <div className="steps" aria-label={`Step ${step + 1} of ${STEPS}`}>
        {Array.from({ length: STEPS }, (_, i) => (
          <i key={i} className={i <= step ? "on" : ""} />
        ))}
      </div>
    </div>
  );
}
