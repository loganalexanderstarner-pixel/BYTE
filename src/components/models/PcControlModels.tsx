import { listen } from "@tauri-apps/api/event";
import { Download, FlaskConical, Square } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { errorText, inTauri } from "../../lib/api";
import { controlApi, planApi, recommendedFirst, sizeText, type CapableModel, type TestScore } from "../../lib/control";
import { useStore } from "../../state/store";

/** What a score means in words. */
export function scoreWords(score: number | null): string {
  if (score == null) return "Not tested yet";
  return `Scored ${score}% on the PC-control test`;
}

/** Runs the PC-control test for a downloaded model and follows its progress. */
export function useSelfTest(onDone: () => void) {
  const [running, setRunning] = useState<string | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!inTauri) return;
    const off = listen<{ done: number; total: number }>("control://selftest", (e) => setProgress({ done: e.payload.done, total: e.payload.total }));
    return () => void off.then((f) => f());
  }, []);
  const start = useCallback(
    async (key: string) => {
      setError(null);
      setRunning(key);
      setProgress(null);
      try {
        await planApi.selftest(key);
      } catch (e) {
        setError(errorText(e));
      }
      setRunning(null);
      onDone();
    },
    [onDone],
  );
  return { running, progress, error, start, stop: () => void planApi.selftestStop() };
}

/**
 * The "PC control" view of the model catalog: the models BYTE lists for controlling the PC (recommended first), with what each is
 * good at, its test score, and a button to use it. "Use for PC control" starts the download first when the model is not here yet;
 * the setting is saved at once and takes effect when the download finishes (until then the chat model plans).
 */
export function PcControlModels() {
  const update = useStore((s) => s.updateSettings);
  const downloads = useStore((s) => s.downloads);
  const [models, setModels] = useState<CapableModel[]>([]);
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    if (inTauri) controlApi.status().then((s) => setModels(recommendedFirst(s.models)), (e) => setError(errorText(e)));
  }, []);
  useEffect(refresh, [refresh]);
  const finished = Object.values(downloads).filter((d) => d?.phase === "finished").length;
  useEffect(refresh, [finished, refresh]);
  const test = useSelfTest(refresh);

  const use = async (m: CapableModel) => {
    setError(null);
    try {
      if (m.installedKey) {
        await update({ controlModel: m.installedKey });
      } else if (m.downloadKey) {
        await update({ controlModel: m.downloadKey });
        await controlApi.download(m.downloadKey);
      }
      refresh();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="section" id="pc-control-models">
      <h4>Models for PC control</h4>
      <small>
        These follow a strict format well, which is what controlling the PC needs. Run the test to see how each one does on 40 standard tasks (nothing is clicked or typed).
      </small>
      {error && <small className="error">{error}</small>}
      {test.error && <small className="error">{test.error}</small>}
      <div className="column" style={{ gap: 10, marginTop: 8 }}>
        {models.map((m) => {
          const d = m.downloadKey ? downloads[m.downloadKey] : undefined;
          const busy = !!d && ["downloading", "resuming", "verifying"].includes(d.phase);
          const testing = test.running === m.installedKey;
          return (
            <div key={m.id} className="row" style={{ gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <span className="grow">
                <strong>{m.name}</strong> {m.recommended && <span className="chip">Recommended</span>} {m.selected && <span className="chip">In use</span>}
                <small style={{ display: "block" }}>
                  {m.note} {m.installedKey ? "Downloaded." : `${sizeText(m.downloadBytes)} to download.`} {scoreWords(m.score)}
                  {testing && test.progress ? ` Testing ${test.progress.done} of ${test.progress.total}…` : ""}
                </small>
              </span>
              {m.installedKey && (
                <button className="btn sm" disabled={!!test.running} onClick={() => void test.start(m.installedKey!)}>
                  <FlaskConical size={14} /> Test
                </button>
              )}
              {testing && (
                <button className="btn sm" onClick={test.stop}>
                  <Square size={14} /> Stop test
                </button>
              )}
              <button className="btn sm primary" disabled={m.selected || busy} onClick={() => void use(m)}>
                {!m.installedKey && <Download size={14} />} {busy ? "Downloading…" : "Use for PC control"}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

export type { TestScore };
