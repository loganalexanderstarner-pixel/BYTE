import { deviceOf, ramLabel } from "../../lib/device";
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
import { DiagnosticsButton } from "../DiagnosticsButton";
import { bytes, contextLabel } from "../../lib/format";
import { findVariant, quantLabel, shortQuant } from "../../lib/models";
import type { ModelStatus, VariantStatus } from "../../lib/types";
import { useStore } from "../../state/store";
import { DownloadProgress, FitPill } from "../models/ModelCard";
import { CloudKeySteps } from "../settings/CloudKeySteps";
import { platformKeys as K, isWindows } from "../../lib/keys";
// --- track:found begin ---
import { controlApi, sizeText, type CapableModel } from "../../lib/control";
import { controlOffer, featurePatch, featuresFor, offerBytes, type FirstRunId } from "../../lib/firstRun";
import { FirstRunFeatures } from "./FirstRunFeatures";
// --- track:found end ---
import { cpuName, graphicsLabel, hardwareNote, isPc, machine, osText } from "../../lib/platform";

const STEPS = 6;
/** Which dot of the progress row a step is on (the cloud step shares the download's place; the feature questions come before the tips). */
const DOT: Record<number, number> = { 0: 0, 1: 1, 2: 2, 3: 3, 5: 3, 6: 4, 4: 5 };

/** Part of the model runs on the CPU instead of the graphics card. */
const spills = (v: VariantStatus) => (v.fit.cpuMoeLayers ?? 0) > 0 || v.fit.gpuLayers != null;

/** Chat models with a version that runs on this Mac, best first. Versions squeezed below 4 bits (a big model
 *  made to fit) come after the good-quality ones, and community remixes (uncensored and other fine-tunes) last,
 *  so a new user's first choices are the makers' own models at good quality.
 *
 *  On a PC (`pc`) the card has its own memory, and a model too big for it spills onto the CPU and system
 *  memory, which is several times slower. Those come after the ones that stay on the card, and BYTE's own
 *  pick (`pc.pick`, what Continue would download) is the first card, so "the first one is BYTE's pick" is
 *  true. A Mac shares one pool of memory, so its order is unchanged. */
export function runnable(
  models: ModelStatus[],
  pc?: { pick: string | null },
): { model: ModelStatus; variant: VariantStatus }[] {
  const tier = (m: ModelStatus, v: VariantStatus) =>
    m.tags.includes("community") || m.tags.includes("uncensored")
      ? 2
      : v.bits < 4
        ? 1
        : 0;
  const slow = (v: VariantStatus) => (pc && spills(v) ? 1 : 0);
  const list = models
    .filter((m) => m.role === "chat" && m.best)
    .map((m) => ({
      model: m,
      variant: m.variants.find((v) => v.key === m.best)!,
    }))
    .sort(
      (a, b) =>
        tier(a.model, a.variant) - tier(b.model, b.variant) ||
        slow(a.variant) - slow(b.variant) ||
        b.variant.quality - a.variant.quality ||
        a.variant.sizeBytes - b.variant.sizeBytes,
    );
  const at = pc?.pick ? list.findIndex((x) => x.variant.key === pc.pick) : -1;
  if (at > 0) list.unshift(...list.splice(at, 1));
  return list;
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

  // --- track:found begin --- the PC-control model offered with the chat model, and the feature questions
  const [offer, setOffer] = useState<CapableModel | null>(null);
  const [withControl, setWithControl] = useState(false);
  const [ticked, setTicked] = useState<FirstRunId[]>([]);
  useEffect(() => {
    if (!isWindows()) return;
    controlApi.status().then((st) => st.supported && setOffer(controlOffer(st.models)), () => undefined);
  }, []);
  // --- track:found end ---

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
      setStep(6);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setConnecting(false);
    }
  };

  // BYTE's pick, asked for here because the store only has it once the engine has started.
  const [pick, setPick] = useState<string | null>(null);
  const haveModels = models.length > 0;
  useEffect(() => {
    if (haveModels) api.modelRecommend().then(setPick, () => undefined);
  }, [haveModels]);

  useEffect(() => {
    if (choice || !models.length) return;
    // Prefer something already downloaded, else BYTE's recommendation.
    const installed = models
      .flatMap((m) => (m.role === "chat" ? m.variants : []))
      .find((v) => v.installed && v.fit.fit !== "toobig");
    if (installed) setChoice(installed.key);
    else {
      const fallback = () => runnable(models, isPc(system) ? { pick: null } : undefined)[0]?.variant.key ?? null;
      api
        .modelRecommend()
        .then((k) => setChoice(k ?? fallback()))
        .catch(() => setChoice(fallback()));
    }
  }, [models, choice, system]);

  const hit = findVariant(models, choice);
  const chosen = hit?.variant;
  const chosenModel = hit?.model;
  const dl = choice ? downloads[choice] : undefined;
  const installed = !!chosen?.installed || dl?.phase === "finished";
  const [showAll, setShowAll] = useState(false);
  const options = runnable(models, isPc(system) ? { pick } : undefined);

  // Move on automatically once the download completes.
  useEffect(() => {
    if (step === 3 && installed) {
      const t = setTimeout(() => setStep(6), 900);
      return () => clearTimeout(t);
    }
  }, [step, installed]);

  const startDownload = async () => {
    if (!choice) return;
    setError(null);
    try {
      if (!chosen?.installed) await api.modelDownload(choice);
      // --- track:found begin ---
      if (withControl && offer?.downloadKey && !offer.installedKey) await api.modelDownload(offer.downloadKey).catch(() => undefined);
      // --- track:found end ---
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
        // --- track:found begin --- only what was ticked; a skipped question leaves the saved setting alone
        ...featurePatch(ticked, withControl && offer ? (offer.installedKey ?? offer.downloadKey) : null),
        // --- track:found end ---
      });
      if (local) void api.modelActivate(choice).catch(() => {});
    } catch (e) {
      setError(errorText(e));
    }
  };

  // Wording that depends on the machine: the backend says which one it is (a PC is never
  // told its hardware is unsupported; that used to be said to every PC).
  const here = machine(system);
  const pc = isPc(system);
  const macNote = useMemo(() => (system ? hardwareNote(system) : null), [system]);
  const device = deviceOf(system);

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
                  A powerful AI assistant that lives entirely on your {here}.
                </p>
                <ul className="tips">
                  <li>
                    <ShieldCheck size={18} />
                    <div>
                      <b>Private by design.</b>{" "}
                      <span className="muted">
                        Your chats and files never leave this {here}. No account,
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
                      <b>Uses your {here}'s hardware.</b>{" "}
                      <span className="muted">
                        {device === "phone" ? "Runs on the phone's own processor, even offline." : pc ? "Runs on your graphics card or processor, even offline." : osText("Runs on the Apple Silicon GPU, even offline.")}
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
                <h1>Checking your {here}</h1>
                <p className="lead">
                  BYTE picks the best model for your hardware.
                </p>
                <div className="spec-grid">
                  <div className="spec">
                    <div className="k">
                      <Cpu size={14} /> {pc ? "Processor" : "Chip"}
                    </div>
                    <div className="v">{pc ? cpuName(system.chip) : system.chip.replace("Apple ", "")}</div>
                  </div>
                  {pc && (
                    <div className="spec">
                      <div className="k">
                        <Cpu size={14} /> Graphics
                      </div>
                      <div className="v">{graphicsLabel(system)}</div>
                    </div>
                  )}
                  <div className="spec">
                    <div className="k">
                      <MemoryStick size={14} /> Memory
                    </div>
                    <div className="v">{ramLabel(system)}</div>
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
                  {system ? ramLabel(system) : ""} {here} — the first
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
                              Best for this {here}
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
                {/* --- track:found begin --- */}
                {offer && (
                  <label className="field control-offer" style={{ marginTop: 12 }}>
                    <span>
                      Also download the model that plans PC control: {offer.name} ({sizeText(offer.downloadBytes)})
                      <small>
                        A small helper model that works out the clicks for "open Notepad". Ticking this only downloads it. PC control stays off until you turn it on in a later question.
                        {offer.installedKey ? " It is already on this PC." : ""}
                        {chosen && !chosen.installed && !offer.installedKey ? ` Together with the chat model: ${bytes(chosen.sizeBytes + offer.downloadBytes)}.` : ""}
                      </small>
                    </span>
                    <input type="checkbox" checked={withControl} onChange={(e) => setWithControl(e.target.checked)} aria-label={`Also download ${offer.name}`} />
                  </label>
                )}
                {/* --- track:found end --- */}
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
                      ? `No model fits this ${here} well. With a BYTE Cloud account, bigger models answer from the cloud; nothing to download.`
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
                      : `Download ${chosen ? bytes(chosen.sizeBytes + (withControl ? offerBytes(offer) : 0)) : ""}`}
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
                    <DiagnosticsButton label="Copy details" />
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
                      onClick={() => setStep(6)}
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
                      {device === "phone" ? (
                        <>
                          <b>Getting around.</b>{" "}
                          <span className="muted">
                            The left button opens your chats, the pencil starts
                            a new one, and ⋯ opens everything else.
                          </span>
                        </>
                      ) : (
                        <>
                          <b>Shortcuts.</b>{" "}
                          <span className="muted">
                            <kbd>{K("⌘N")}</kbd> new chat · <kbd>{K("⌘,")}</kbd> settings ·{" "}
                            <kbd>{K("⌘\\")}</kbd> sidebar · <kbd>Esc</kbd> stop
                          </span>
                        </>
                      )}
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

            {/* --- track:found begin --- */}
            {step === 6 && (
              <>
                <FirstRunFeatures
                  features={featuresFor(isWindows(), device === "phone")}
                  ticked={ticked}
                  onChange={setTicked}
                  controlModel={withControl && offer ? offer.name : null}
                />
                <div className="onb-actions">
                  <button className="btn ghost" onClick={() => setStep(viaCloud ? 5 : 3)}>
                    Back
                  </button>
                  <button className="btn lg primary" onClick={() => setStep(4)}>
                    {ticked.length ? "Continue" : "Continue with all of these off"}
                  </button>
                </div>
              </>
            )}
            {/* --- track:found end --- */}

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
                  className="field field-row"
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
                    onClick={() => setStep(6)}
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
        aria-label={`Step ${(DOT[step] ?? 0) + 1} of ${STEPS}`}
      >
        {Array.from({ length: STEPS }, (_, i) => (
          <i key={i} className={i <= (DOT[step] ?? 0) ? "on" : ""} />
        ))}
      </div>
    </div>
  );
}
