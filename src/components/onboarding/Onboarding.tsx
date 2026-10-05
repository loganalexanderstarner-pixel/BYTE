import { AnimatePresence, motion } from "framer-motion";
import {
  ArrowUp,
  Brain,
  CircleCheck,
  Cloud,
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
import { findVariant, quantLabel, shortQuant } from "../../lib/models";
import type { ModelStatus, VariantStatus } from "../../lib/types";
import { useStore } from "../../state/store";
import { DownloadProgress, FitPill } from "../models/ModelCard";
import { CloudKeySteps } from "../settings/CloudKeySteps";
import { platformKeys as K } from "../../lib/keys";

const STEPS = 5;

/** Chat models with a version that runs on this Mac, best first. Versions squeezed below 4 bits (a big model
 *  made to fit) come after the good-quality ones, and community remixes (uncensored and other fine-tunes) last,
 *  so a new user's first choices are the makers' own models at good quality. */
export function runnable(
  models: ModelStatus[],
): { model: ModelStatus; variant: VariantStatus }[] {
  const tier = (m: ModelStatus, v: VariantStatus) =>
    m.tags.includes("community") || m.tags.includes("uncensored")
      ? 2
      : v.bits < 4
        ? 1
        : 0;
  return models
    .filter((m) => m.role === "chat" && m.best)
    .map((m) => ({
      model: m,
      variant: m.variants.find((v) => v.key === m.best)!,
    }))
    .sort(
      (a, b) =>
        tier(a.model, a.variant) - tier(b.model, b.variant) ||
        b.variant.quality - a.variant.quality ||
        a.variant.sizeBytes - b.variant.sizeBytes,
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
  const [name, setName] = useState("");
  const [viaCloud, setViaCloud] = useState(false);
  const [cloudKey, setCloudKey] = useState("");
  const [connecting, setConnecting] = useState(false);
  const setWorkspace = useStore((s) => s.setWorkspace);
  const refreshCloud = useStore((s) => s.refreshCloud);

  const connectCloud = async () => {
    setError(null);
    setConnecting(true);
    try {
      await api.cloudConnect(cloudKey, null);
      setCloudKey(""); // kept only in the Keychain from here on
      useStore.setState({ settings: await api.settingsGet() });
      await refreshCloud();
      await setWorkspace("cloud");
      setViaCloud(true);
      setStep(4);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setConnecting(false);
    }
  };

  useEffect(() => {
    if (choice || !models.length) return;
    // Prefer something already downloaded, else BYTE's recommendation.
    const installed = models
      .flatMap((m) => (m.role === "chat" ? m.variants : []))
      .find((v) => v.installed && v.fit.fit !== "toobig");
    if (installed) setChoice(installed.key);
    else {
      const fallback = () => runnable(models)[0]?.variant.key ?? null;
      api
        .modelRecommend()
        .then((k) => setChoice(k ?? fallback()))
        .catch(() => setChoice(fallback()));
    }
  }, [models, choice]);

  const hit = findVariant(models, choice);
  const chosen = hit?.variant;
  const chosenModel = hit?.model;
  const dl = choice ? downloads[choice] : undefined;
  const installed = !!chosen?.installed || dl?.phase === "finished";
  const [showAll, setShowAll] = useState(false);
  const options = runnable(models);

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
      const local = !viaCloud && choice;
      await update({
        onboardingComplete: true,
        ...(local ? { activeModel: choice } : {}),
        userName: name.trim() || null,
      });
      if (local) void api.modelActivate(choice).catch(() => {});
    } catch (e) {
      setError(errorText(e));
    }
  };

  const macNote = useMemo(() => {
    if (!system) return null;
    const gb = system.totalRamBytes / 2 ** 30;
    if (!system.appleSilicon)
      return {
        ok: false,
        text: "BYTE is built for Apple Silicon Macs (M1 or newer). It may not run well here.",
      };
    if (gb < 12)
      return {
        ok: false,
        text: "With 8 GB of memory, BYTE will use the smaller Fast model.",
      };
    if (gb < 20)
      return {
        ok: true,
        text: "Your Mac can run BYTE's Smart model comfortably.",
      };
    return {
      ok: true,
      text: "Your Mac has plenty of memory — every BYTE model will run well.",
    };
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
                <p className="lead">
                  A powerful AI assistant that lives entirely on your Mac.
                </p>
                <ul className="tips">
                  <li>
                    <ShieldCheck size={18} />
                    <div>
                      <b>Private by design.</b>{" "}
                      <span className="muted">
                        Your chats and files never leave this Mac. No account,
                        no subscription.
                      </span>
                    </div>
                  </li>
                  <li>
                    <Brain size={18} />
                    <div>
                      <b>Thinks things through.</b>{" "}
                      <span className="muted">
                        Turn on thinking for hard questions and watch it reason
                        step by step.
                      </span>
                    </div>
                  </li>
                  <li>
                    <Cpu size={18} />
                    <div>
                      <b>Uses your Mac's hardware.</b>{" "}
                      <span className="muted">
                        Runs on the Apple Silicon GPU, even offline.
                      </span>
                    </div>
                  </li>
                </ul>
                <div className="onb-actions">
                  <span />
                  <button className="btn lg primary" onClick={() => setStep(1)}>
                    Get started
                  </button>
                </div>
              </>
            )}

            {step === 1 && system && (
              <>
                <h1>Checking your Mac</h1>
                <p className="lead">
                  BYTE picks the best model for your hardware.
                </p>
                <div className="spec-grid">
                  <div className="spec">
                    <div className="k">
                      <Cpu size={14} /> Chip
                    </div>
                    <div className="v">{system.chip.replace("Apple ", "")}</div>
                  </div>
                  <div className="spec">
                    <div className="k">
                      <MemoryStick size={14} /> Memory
                    </div>
                    <div className="v">{ramSize(system.totalRamBytes)}</div>
                  </div>
                  <div className="spec">
                    <div className="k">
                      <HardDrive size={14} /> Free disk
                    </div>
                    <div className="v">{bytes(system.freeDiskBytes, 0)}</div>
                  </div>
                </div>
                {macNote && (
                  <div
                    className={`banner ${macNote.ok ? "" : "danger"}`}
                    style={{ marginTop: 16 }}
                  >
                    {macNote.ok ? (
                      <CircleCheck size={18} style={{ color: "var(--ok)" }} />
                    ) : (
                      <TriangleAlert
                        size={18}
                        style={{ color: "var(--warn)" }}
                      />
                    )}
                    <span className="grow">{macNote.text}</span>
                  </div>
                )}
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(0)}>
                    Back
                  </button>
                  <button className="btn lg primary" onClick={() => setStep(2)}>
                    Continue
                  </button>
                </div>
              </>
            )}

            {step === 2 && (
              <>
                <h1>Choose your model</h1>
                <p className="lead">
                  This is BYTE's brain. These run well on your{" "}
                  {system ? ramSize(system.totalRamBytes) : ""} Mac — the first
                  one is BYTE's pick. You can try others any time in Settings →
                  Models.
                </p>
                <div className="model-list">
                  {(showAll ? options : options.slice(0, 4)).map(
                    ({ model: m, variant: v }, i) => (
                      <button
                        key={v.key}
                        className="model-card selectable"
                        aria-pressed={choice === v.key}
                        onClick={() => setChoice(v.key)}
                      >
                        <div className="title">
                          {m.name}
                          {i === 0 && (
                            <span className="pill accent">
                              Best for this Mac
                            </span>
                          )}
                          {v.installed && (
                            <span className="pill ok">Downloaded</span>
                          )}
                        </div>
                        <div className="muted" style={{ fontSize: "0.92em" }}>
                          {m.tagline}
                        </div>
                        <div className="meta">
                          <span>{bytes(v.sizeBytes)} download</span>
                          <span>· {quantLabel(v.bits)}</span>
                          {m.thinking && <span>· Thinking</span>}
                          <span style={{ marginLeft: "auto" }}>
                            <FitPill v={v} />
                          </span>
                        </div>
                      </button>
                    ),
                  )}
                </div>
                {options.length > 4 && (
                  <button
                    className="btn sm ghost"
                    style={{ marginTop: 8 }}
                    onClick={() => setShowAll(!showAll)}
                  >
                    {showAll
                      ? "Show fewer"
                      : `Show all ${options.length} models that fit`}
                  </button>
                )}
                {system &&
                  chosen &&
                  !chosen.installed &&
                  system.freeDiskBytes > 0 &&
                  system.freeDiskBytes < chosen.sizeBytes + 1e9 && (
                    <div className="banner danger" style={{ marginTop: 12 }}>
                      <TriangleAlert size={18} />
                      <span className="grow">
                        Not enough free disk space for this model. Free up{" "}
                        {bytes(chosen.sizeBytes + 1e9 - system.freeDiskBytes)}{" "}
                        and try again.
                      </span>
                    </div>
                  )}
                {error && (
                  <div className="banner danger" style={{ marginTop: 12 }}>
                    {error}
                  </div>
                )}
                <button
                  className={`model-card selectable cloud-choice ${options.length === 0 || (system && system.totalRamBytes < 12 * 2 ** 30) ? "suggested" : ""}`}
                  onClick={() => {
                    setError(null);
                    setStep(5);
                  }}
                >
                  <div className="title">
                    <Cloud size={16} /> Use BYTE Cloud instead
                    <span className="pill">Invite only</span>
                  </div>
                  <div className="muted" style={{ fontSize: "0.92em" }}>
                    {options.length === 0
                      ? "No model fits this Mac well. With a BYTE Cloud account, bigger models answer from the cloud; nothing to download."
                      : "Answers come from bigger models on the BYTE cloud; nothing to download. Needs an account (by invite) and an API key."}
                  </div>
                </button>
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(1)}>
                    Back
                  </button>
                  <button
                    className="btn lg primary"
                    onClick={startDownload}
                    disabled={!chosen}
                  >
                    {chosen?.installed
                      ? "Continue"
                      : `Download ${chosen ? bytes(chosen.sizeBytes) : ""}`}
                  </button>
                </div>
              </>
            )}

            {step === 3 && chosen && chosenModel && (
              <>
                <h1>
                  {installed ? "All set." : `Downloading ${chosenModel.name}`}
                </h1>
                <p className="lead">
                  {installed
                    ? "The model is verified and ready."
                    : "This is a one-time download. You can pause and resume any time — even after quitting."}
                </p>
                <div className="model-card">
                  <div className="title">
                    {chosenModel.name}{" "}
                    <span className="faint" style={{ fontWeight: 500 }}>
                      {shortQuant(chosen.quant)}
                    </span>
                  </div>
                  <DownloadProgress
                    variant={chosen}
                    dl={
                      dl ??
                      (installed
                        ? {
                            phase: "finished",
                            bytes: chosen.sizeBytes,
                            total: chosen.sizeBytes,
                            bytesPerSec: 0,
                          }
                        : undefined)
                    }
                  />
                </div>
                {dl?.phase === "failed" && (
                  <div className="banner danger" style={{ marginTop: 12 }}>
                    <span className="grow">{dl.error}</span>
                    <button className="btn sm" onClick={startDownload}>
                      Retry
                    </button>
                  </div>
                )}
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(2)}>
                    Back
                  </button>
                  <div className="row">
                    {!installed && dl?.phase === "paused" && (
                      <button className="btn" onClick={startDownload}>
                        Resume
                      </button>
                    )}
                    {!installed &&
                      dl?.phase !== "paused" &&
                      dl?.phase !== "failed" && (
                        <button
                          className="btn"
                          onClick={() => void api.modelPause(chosen.key)}
                        >
                          Pause
                        </button>
                      )}
                    <button
                      className="btn lg primary"
                      onClick={() => setStep(4)}
                      disabled={!installed}
                    >
                      Continue
                    </button>
                  </div>
                </div>
              </>
            )}

            {step === 5 && (
              <>
                <h1>Connect BYTE Cloud</h1>
                <p className="lead">
                  Answers come from bigger models on the BYTE cloud. You need a
                  key from your BYTE account:
                </p>
                <CloudKeySteps />
                <div className="row" style={{ gap: 8 }}>
                  <input
                    className="text-input key-input"
                    style={{ flex: 1 }}
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="byte_…"
                    value={cloudKey}
                    onChange={(e) => setCloudKey(e.target.value)}
                    onKeyDown={(e) =>
                      e.key === "Enter" &&
                      cloudKey.trim() &&
                      void connectCloud()
                    }
                    aria-label="BYTE cloud API key"
                  />
                </div>
                <p className="faint" style={{ fontSize: "0.88em" }}>
                  No invite? Pick a model instead: everything runs on this Mac.
                  You can connect the cloud later in Settings → Cloud.
                </p>
                {error && <div className="banner danger">{error}</div>}
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(2)}>
                    Back
                  </button>
                  <button
                    className="btn lg primary"
                    onClick={() => void connectCloud()}
                    disabled={!cloudKey.trim() || connecting}
                  >
                    <Cloud size={16} /> {connecting ? "Checking…" : "Connect"}
                  </button>
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
                        <Zap size={12} /> Fast for quick answers,{" "}
                        <Gauge size={12} /> Auto decides for you,{" "}
                        <Telescope size={12} /> Deep for thorough answers,{" "}
                        <Rocket size={12} /> Extended for the most complete
                        work.
                      </span>
                    </div>
                  </li>
                  <li>
                    <Brain size={18} />
                    <div>
                      <b>Thinking.</b>{" "}
                      <span className="muted">
                        Click the thinking button to force it on or off. Open
                        “Thought for…” to see how BYTE reasoned.
                      </span>
                    </div>
                  </li>
                  <li>
                    <Keyboard size={18} />
                    <div>
                      <b>Shortcuts.</b>{" "}
                      <span className="muted">
                        <kbd>{K("⌘N")}</kbd> new chat · <kbd>{K("⌘,")}</kbd> settings ·{" "}
                        <kbd>{K("⌘\\")}</kbd> sidebar · <kbd>Esc</kbd> stop
                      </span>
                    </div>
                  </li>
                  <li>
                    <Sparkles size={18} />
                    <div>
                      <b>Stays up to date.</b>{" "}
                      <span className="muted">
                        With <b>Web</b> on, BYTE searches and reads the web for
                        current questions and shows numbered sources you can
                        click.
                      </span>
                    </div>
                  </li>
                </ul>
                <div
                  className="field"
                  style={{ marginTop: 12, borderBottom: 0 }}
                >
                  <label>
                    What should BYTE call you?
                    <small>Optional — used for friendly greetings.</small>
                  </label>
                  <input
                    className="text-input"
                    value={name}
                    maxLength={40}
                    placeholder="Your name"
                    onChange={(e) => setName(e.target.value)}
                  />
                </div>
                {!viaCloud && chosen && chosenModel && (
                  <p
                    className="faint"
                    style={{ fontSize: "0.88em", marginTop: 16 }}
                  >
                    {chosenModel.name} will use about{" "}
                    {bytes(chosen.fit.neededBytes)} of memory with a{" "}
                    {contextLabel(chosen.fit.context)}-token context.
                  </p>
                )}
                {error && <div className="banner danger">{error}</div>}
                <div className="onb-actions">
                  <button
                    className="btn ghost"
                    onClick={() => setStep(viaCloud ? 5 : 3)}
                  >
                    Back
                  </button>
                  <button className="btn lg primary" onClick={finish}>
                    Start chatting{" "}
                    <ArrowUp size={16} style={{ transform: "rotate(90deg)" }} />
                  </button>
                </div>
              </>
            )}
          </motion.div>
        </AnimatePresence>
      </div>
      <div
        className="steps"
        aria-label={`Step ${Math.min(step, STEPS - 1) + 1} of ${STEPS}`}
      >
        {Array.from({ length: STEPS }, (_, i) => (
          <i key={i} className={i <= step ? "on" : ""} />
        ))}
      </div>
    </div>
  );
}
