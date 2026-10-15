import { Check, Download, Pause, Play, Search, Sparkles, Trash2, Volume2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { bytes } from "../../lib/format";
import type { VoicePackage, VoicesStatus } from "../../lib/types";
import { filterVoices, languages, NO_FILTER, pick, providers, ramLabel, speakerLine, speakersFor, voiceSetting, type VoiceFilter } from "../../lib/voices";
import { useStore, type DownloadState } from "../../state/store";
import { osText } from "../../lib/platform";

const busy = (d?: DownloadState) => !!d && (d.phase === "downloading" || d.phase === "resuming" || d.phase === "verifying");
const PAGE = 12;
const SAMPLE = "Hi, I'm BYTE. Here's how I sound when I read my answers to you.";
/** A short recording of each provider, bundled with the app (public/voices), to hear before downloading. */
const PREVIEW: Record<string, string> = {
  Kokoro: "voices/kokoro.wav",
  Piper: "voices/piper.wav",
  "Kitten TTS": "voices/kitten.wav",
  Supertonic: "voices/supertonic.wav",
  "Pocket TTS (Kyutai)": "voices/pocket.wav",
};

export const keyOf = (p: VoicePackage) => (p.id === "kokoro-v1_0" ? "voice:kokoro" : `voice:pkg:${p.id}`);

/** Settings → Models → Voice → Browse voices: every free voice BYTE can use, by provider, with filters. */
export function VoiceBrowser({ catalog, status, onChanged }: { catalog: VoicePackage[]; status: VoicesStatus | null; onChanged: () => void }) {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const downloads = useStore((s) => s.downloads);
  const [f, setF] = useState<VoiceFilter>(NO_FILTER);
  const [shown, setShown] = useState(PAGE);
  const [error, setError] = useState<string | null>(null);
  const [unpacking, setUnpacking] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const audio = useRef<HTMLAudioElement | null>(null);
  const ready = status?.ready ?? [];
  const current = pick(catalog, settings?.byteVoice);
  const list = useMemo(() => filterVoices(catalog, f, ready), [catalog, f, ready]);
  const langs = useMemo(() => languages(catalog), [catalog]);
  const provs = useMemo(() => providers(catalog), [catalog]);
  const total = useMemo(() => catalog.reduce((n, p) => n + p.speakers.length, 0), [catalog]);

  // A finished download is unpacked, then it's ready.
  useEffect(() => {
    const done = catalog.find((p) => (downloads[keyOf(p)]?.phase === "finished" || status?.downloaded.includes(p.id)) && !ready.includes(p.id));
    if (!done || unpacking) return;
    setUnpacking(done.id);
    void api
      .ttsVoiceUnpack(done.id)
      .then(onChanged, (e) => setError(errorText(e)))
      .finally(() => setUnpacking(null));
  }, [downloads, status, catalog, ready, unpacking, onChanged]);

  const hear = (p: VoicePackage) => {
    audio.current?.pause();
    if (preview === p.provider) return setPreview(null);
    const a = new Audio(PREVIEW[p.provider]);
    audio.current = a;
    a.onended = () => setPreview(null);
    setPreview(p.provider);
    void a.play().catch(() => setPreview(null));
  };
  const run = (fn: () => Promise<unknown>) => {
    setError(null);
    void fn().then(onChanged, (e) => setError(errorText(e)));
  };
  const set = (patch: Partial<VoiceFilter>) => {
    setF({ ...f, ...patch });
    setShown(PAGE);
  };

  return (
    <div className="voice-browser">
      <p className="faint" style={{ margin: "0 0 8px", fontSize: "0.88em" }}>
        {catalog.length} free voice downloads with {total.toLocaleString()} voices from {provs.length} {osText("open projects. All made on this Mac by BYTE's speech engine: no account, nothing sent anywhere. They run on the CPU, not the GPU your chat model uses, and only while BYTE is talking.")}
      </p>
      <div className="catalog-tools">
        <label className="search">
          <Search size={14} className="faint" />
          <input value={f.query} onChange={(e) => set({ query: e.target.value })} placeholder="Search voices (e.g. British, calm, Michael)" aria-label="Search voices" />
        </label>
        <select value={f.lang} onChange={(e) => set({ lang: e.target.value })} aria-label="Language">
          <option value="">Any language</option>
          {langs.map(([code, name, n]) => (
            <option key={code} value={code}>
              {name} ({n})
            </option>
          ))}
        </select>
      </div>
      <div className="filters">
        <div className="chips" role="group" aria-label="Voice">
          <button className="chip" aria-pressed={!f.gender} onClick={() => set({ gender: "" })}>Any voice</button>
          <button className="chip" aria-pressed={f.gender === "female"} onClick={() => set({ gender: f.gender === "female" ? "" : "female" })}>Female</button>
          <button className="chip" aria-pressed={f.gender === "male"} onClick={() => set({ gender: f.gender === "male" ? "" : "male" })}>Male</button>
          <button className="chip" aria-pressed={f.expressive} onClick={() => set({ expressive: !f.expressive })} title="Voices with more feeling and natural rhythm">
            <Sparkles size={12} /> Expressive
          </button>
          <button className="chip" aria-pressed={f.light} onClick={() => set({ light: !f.light })} title="200 MB of memory or less while speaking: good next to a big model">
            Light on memory
          </button>
          <button className="chip" aria-pressed={f.downloaded} onClick={() => set({ downloaded: !f.downloaded })}>Downloaded ({ready.length})</button>
        </div>
        <div className="chips" role="group" aria-label="Provider">
          <button className="chip" aria-pressed={!f.provider} onClick={() => set({ provider: "" })}>All providers</button>
          {provs.map((p) => (
            <button key={p} className="chip" aria-pressed={f.provider === p} onClick={() => set({ provider: f.provider === p ? "" : p })}>{p}</button>
          ))}
        </div>
      </div>
      {error && <div className="banner danger">{error}</div>}
      {list.length === 0 && <p className="faint">No voices match these filters.</p>}
      <div className="model-list">
        {list.slice(0, shown).map((p) => {
          const d = downloads[keyOf(p)];
          const isReady = ready.includes(p.id);
          const speakers = speakersFor(p, f);
          const pct = d?.total ? Math.round((d.bytes / d.total) * 100) : 0;
          return (
            <div key={p.id} className={`model-card voice-card ${current?.pkg.id === p.id ? "active" : ""}`}>
              <div className="row" style={{ alignItems: "flex-start" }}>
                <div className="grow">
                  <div className="title">
                    {p.name} {!p.name.startsWith(p.provider.split(" ")[0]) && <span className="pill">{p.provider}</span>}
                    {p.expressive && <span className="pill ok"><Sparkles size={11} /> Expressive</span>}
                    {isReady && <span className="pill ok"><Check size={11} /> Downloaded</span>}
                  </div>
                  <div className="faint" style={{ fontSize: "0.88em", margin: "2px 0 4px" }}>{p.about}</div>
                  <div className="faint" style={{ fontSize: "0.8em" }}>
                    {p.languageName} · {p.speakers.length} voice{p.speakers.length === 1 ? "" : "s"} · {bytes(p.size)} download · about {ramLabel(p.ramMb)} of memory while speaking · {p.quality} quality · {p.license}
                  </div>
                </div>
                <div className="row" style={{ gap: 6 }}>
                  {PREVIEW[p.provider] && !isReady && (
                    <button className="btn sm ghost" onClick={() => hear(p)} title={`Hear a ${p.provider} voice`}>
                      {preview === p.provider ? <Pause size={13} /> : <Play size={13} />} Sample
                    </button>
                  )}
                  {isReady ? (
                    <button className="icon-btn sm" aria-label={`Delete ${p.name}`} title="Delete this voice download" onClick={() => run(() => api.ttsVoiceDelete(p.id))}>
                      <Trash2 size={13} />
                    </button>
                  ) : unpacking === p.id ? (
                    <span className="faint small">Unpacking…</span>
                  ) : busy(d) ? (
                    <span className="faint small">{pct}%</span>
                  ) : (
                    <button className="btn sm primary" disabled={!inTauri} onClick={() => run(() => api.ttsVoiceDownload(p.id))}>
                      <Download size={13} /> {d?.phase === "paused" ? "Resume" : "Download"}
                    </button>
                  )}
                </div>
              </div>
              {busy(d) && (
                <span className="progress" style={{ display: "block", marginTop: 6 }} aria-label={`Downloading ${pct}%`}>
                  <span style={{ width: `${pct}%` }} />
                </span>
              )}
              {isReady && (
                <div className="voice-speakers">
                  {speakers.length > 30 ? (
                    <select
                      aria-label={`${p.name} voice`}
                      value={current?.pkg.id === p.id ? current.speaker.id : ""}
                      onChange={(e) => void update({ byteVoice: voiceSetting(p, p.speakers.find((s) => s.id === e.target.value) ?? p.speakers[0]) })}
                    >
                      <option value="" disabled>Choose one of {speakers.length} voices…</option>
                      {speakers.map((s) => (
                        <option key={s.id} value={s.id}>{s.name}{s.gender ? ` (${s.gender})` : ""}</option>
                      ))}
                    </select>
                  ) : (
                    speakers.map((s) => {
                      const on = current?.pkg.id === p.id && current.speaker.id === s.id;
                      return (
                        <span key={s.id} className={`voice-speaker ${on ? "on" : ""}`} title={s.about || speakerLine(s)}>
                          <button className="linklike" onClick={() => void update({ byteVoice: voiceSetting(p, s) })} aria-pressed={on}>
                            {on && <Check size={11} />} {s.name}
                          </button>
                          <button className="icon-btn sm" aria-label={`Hear ${s.name}`} onClick={() => run(() => api.speechSay(SAMPLE, voiceSetting(p, s)))}>
                            <Volume2 size={12} />
                          </button>
                        </span>
                      );
                    })
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
      {list.length > shown && (
        <button className="btn sm ghost" style={{ marginTop: 8 }} onClick={() => setShown(shown + PAGE)}>
          Show {Math.min(PAGE, list.length - shown)} more
        </button>
      )}
    </div>
  );
}
