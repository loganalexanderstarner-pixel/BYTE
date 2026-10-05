import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import { Brain, FileUp, FlaskConical, Link2, Loader2, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { bytes, contextLabel } from "../../lib/format";
import { fitLabel, fitTone, paramsLabel } from "../../lib/tuning";
import type { LabModel } from "../../lib/types";
import { useStore } from "../../state/store";
import { osText } from "../../lib/platform";

/** "Model lab": add any GGUF model, from a file on this Mac or a Hugging Face link. */
export function ModelLab() {
  const refreshModels = useStore((s) => s.refreshModels);
  const [url, setUrl] = useState("");
  const [inspected, setInspected] = useState<LabModel | null>(null);
  const [added, setAdded] = useState<LabModel[]>([]);
  const [busy, setBusy] = useState<"file" | "url" | "add" | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadList = () =>
    api
      .labList()
      .then(setAdded)
      .catch(() => setAdded([]));

  useEffect(() => {
    void loadList();
  }, []);

  const run = async (kind: "file" | "url" | "add", fn: () => Promise<void>) => {
    setError(null);
    setNote(null);
    setBusy(kind);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    setBusy(null);
  };

  const chooseFile = () =>
    run("file", async () => {
      const picked: unknown = await openDialog({ title: "Choose a GGUF model file", filters: [{ name: "GGUF model", extensions: ["gguf"] }] });
      const path = typeof picked === "string" ? picked : Array.isArray(picked) && typeof picked[0] === "string" ? picked[0] : null;
      if (!path) return;
      setInspected(await api.labInspect(path));
    });

  const checkUrl = () =>
    run("url", async () => {
      const link = url.trim();
      if (!link) return;
      setInspected(await api.labInspectUrl(link));
    });

  const add = (model: LabModel) =>
    run("add", async () => {
      const key = await api.labAdd(model);
      if (model.source === "huggingface") {
        await api.modelDownload(key);
        setNote(`Downloading ${model.name} — see Models above.`);
      } else {
        setNote(`${model.name} is in your model list.`);
      }
      setInspected(null);
      setUrl("");
      await Promise.all([loadList(), refreshModels()]);
    });

  const remove = async (model: LabModel) => {
    const ok = await ask(osText(`Remove ${model.name} from BYTE? ${model.source === "huggingface" ? "Its downloaded file is deleted too." : "The file on your Mac stays where it is."}`), {
      title: "Remove model",
      kind: "warning",
    });
    if (!ok) return;
    setError(null);
    setNote(null);
    try {
      await api.labRemove(model.id);
      await Promise.all([loadList(), refreshModels()]);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="section model-lab">
      <h4>
        <FlaskConical size={13} /> Model lab
      </h4>
      <p className="muted" style={{ marginTop: 0 }}>
        {osText("Try any model in the GGUF format. BYTE reads the file's details and tells you whether it fits this Mac before you add it.")}
      </p>
      {error && <div className="banner danger">{error}</div>}
      {note && (
        <div className="banner" role="status">
          {note}
        </div>
      )}

      <div className="field">
        <label>
          {osText("A file on this Mac")}
          <small>A .gguf file you already downloaded. BYTE uses it where it is.</small>
        </label>
        <button className="btn sm" onClick={() => void chooseFile()} disabled={busy !== null}>
          {busy === "file" ? <Loader2 size={14} className="spin" /> : <FileUp size={14} />} Choose a GGUF file…
        </button>
      </div>
      <form
        className="lab-url"
        onSubmit={(e) => {
          e.preventDefault();
          void checkUrl();
        }}
      >
        <label htmlFor="lab-url">Or paste a Hugging Face link to a .gguf file</label>
        <div className="row" style={{ gap: 8 }}>
          <input
            id="lab-url"
            className="text-input grow"
            type="url"
            placeholder="https://huggingface.co/…/model-Q4_K_M.gguf"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            spellCheck={false}
          />
          <button className="btn sm" type="submit" disabled={busy !== null || !url.trim()}>
            {busy === "url" ? <Loader2 size={14} className="spin" /> : <Link2 size={14} />} Check
          </button>
        </div>
      </form>

      {inspected && <LabCard model={inspected} busy={busy === "add"} onAdd={() => void add(inspected)} onDismiss={() => setInspected(null)} />}

      <h4 style={{ marginTop: 20 }}>Models you added ({added.length})</h4>
      {added.length === 0 ? (
        <p className="faint" style={{ margin: 0 }}>
          None yet.
        </p>
      ) : (
        <div className="lab-list">
          {added.map((m) => (
            <div key={m.id} className="lab-row">
              <div className="grow">
                <b>{m.name}</b>
                <span className="faint">
                  {m.architecture} · {paramsLabel(m.paramsB)} · {m.quant} · {bytes(m.sizeBytes)} · {m.source === "huggingface" ? "Hugging Face" : osText("file on this Mac")}
                </span>
              </div>
              <span className={`pill ${fitTone(m.fit)}`}>{fitLabel(m.fit)}</span>
              <button className="btn sm ghost" onClick={() => void remove(m)} title={`Remove ${m.name}`} aria-label={`Remove ${m.name}`}>
                <Trash2 size={14} /> Remove
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function LabCard({ model, busy, onAdd, onDismiss }: { model: LabModel; busy: boolean; onAdd: () => void; onDismiss: () => void }) {
  const tone = fitTone(model.fit);
  return (
    <div className={`card lab-card ${tone}`}>
      <div className="lab-card-head">
        <div className="grow">
          <b className="lab-name">{model.name}</b>
          <span className="faint lab-source">{model.source === "huggingface" ? `${model.repo} · ${model.file}` : model.path}</span>
        </div>
        <span className={`pill ${tone}`}>{fitLabel(model.fit)}</span>
      </div>
      <dl className="lab-facts">
        <div>
          <dt>Architecture</dt>
          <dd>{model.architecture || "unknown"}</dd>
        </div>
        <div>
          <dt>Parameters</dt>
          <dd>{paramsLabel(model.paramsB)}</dd>
        </div>
        <div>
          <dt>Quantization</dt>
          <dd>{model.quant || "unknown"}</dd>
        </div>
        <div>
          <dt>Size</dt>
          <dd>{bytes(model.sizeBytes)}</dd>
        </div>
        <div>
          <dt>Max context</dt>
          <dd>{model.contextMax > 0 ? `${contextLabel(model.contextMax)} tokens` : "unknown"}</dd>
        </div>
        {model.thinking && (
          <div>
            <dt>Thinking</dt>
            <dd className="row" style={{ gap: 4 }}>
              <Brain size={13} /> Can think
            </dd>
          </div>
        )}
      </dl>
      <p className={`lab-fit-note ${tone}`}>{model.fitNote}</p>
      <div className="row" style={{ justifyContent: "flex-end", gap: 8 }}>
        <button className="btn sm ghost" onClick={onDismiss}>
          Cancel
        </button>
        <button className="btn sm primary" onClick={onAdd} disabled={busy || model.fit === "no" || model.added} title={model.fit === "no" ? osText("Too big to run on this Mac") : undefined}>
          {busy ? <Loader2 size={14} className="spin" /> : <Plus size={14} />} {model.added ? "Already added" : "Add to BYTE"}
        </button>
      </div>
    </div>
  );
}
