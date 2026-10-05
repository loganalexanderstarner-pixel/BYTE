import { Download, RefreshCw, Trash2, Volume2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { bytes } from "../../lib/format";
import type { CloudVoice, MediaStatus, SpeakersStatus, SpeechVoice, VoicePackage, VoicesStatus } from "../../lib/types";
import { pick } from "../../lib/voices";
import { canSpeak, useStore, type DownloadState } from "../../state/store";
import { VoiceBrowser } from "./VoiceBrowser";
import { osText } from "../../lib/platform";

const busy = (d?: DownloadState) => !!d && (d.phase === "downloading" || d.phase === "resuming" || d.phase === "verifying");
const pct = (ds: (DownloadState | undefined)[]) => {
  const total = ds.reduce((n, d) => n + (d?.total ?? 0), 0);
  return total ? Math.round((ds.reduce((n, d) => n + (d?.bytes ?? 0), 0) / total) * 100) : 0;
};

/** Settings → Models → Voice: who's speaking in recordings (speakers.rs). */
export function SpeakerLabelsRow() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const downloads = useStore((s) => s.downloads);
  const [st, setSt] = useState<SpeakersStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => void (inTauri ? api.speakersStatus() : Promise.resolve(null)).then(setSt, (e) => setError(errorText(e)));
  useEffect(refresh, []);
  const ds = (st?.keys ?? []).map((k) => downloads[k]);
  const done = ds.length > 0 && ds.every((d) => d?.phase === "finished");
  useEffect(() => {
    if (done) refresh();
  }, [done]);
  const on = settings?.voiceSpeakers !== false;
  const loading = ds.some(busy);
  return (
    <div className="field">
      <span>
        <label style={{ cursor: "pointer" }}>
          <input type="checkbox" checked={on} onChange={(e) => void update({ voiceSpeakers: e.target.checked })} /> Label who's speaking in recordings
        </label>
        <small>
          Transcripts of meetings, interviews and calls show “Speaker 1”, “Speaker 2”… with times, so you can ask what each person said. Needs two small models{st ? ` (${bytes(st.sizeBytes)})` : ""}.
        </small>
        {loading && (
          <span className="progress" aria-label={`Downloading ${pct(ds)}%`} style={{ display: "block", marginTop: 6 }}>
            <span style={{ width: `${pct(ds)}%` }} />
          </span>
        )}
        {error && <small className="bad">{error}</small>}
        {ds.find((d) => d?.phase === "failed")?.error && <small className="bad">{ds.find((d) => d?.phase === "failed")?.error}</small>}
      </span>
      {st?.installed ? (
        <button className="icon-btn sm" title="Delete the speaker-label models" aria-label="Delete the speaker-label models" onClick={() => void api.speakersDelete().then(refresh, (e) => setError(errorText(e)))}>
          <Trash2 size={13} />
        </button>
      ) : loading ? (
        <span className="faint small">{pct(ds)}%</span>
      ) : (
        <button className="btn sm primary" disabled={!on} onClick={() => void api.speakersDownload().catch((e) => setError(errorText(e)))}>
          <Download size={13} /> Download
        </button>
      )}
    </div>
  );
}

/** Settings → Models → Voice: the video helper (yt-dlp) for YouTube videos without captions (media.rs). */
export function VideoHelperRow() {
  const downloads = useStore((s) => s.downloads);
  const [st, setSt] = useState<MediaStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const refresh = () => void (inTauri ? api.mediaStatus() : Promise.resolve(null)).then(setSt, (e) => setError(errorText(e)));
  useEffect(refresh, []);
  const d = st ? downloads[st.key] : undefined;
  useEffect(() => {
    if (d?.phase === "finished") refresh();
  }, [d?.phase]);
  const loading = busy(d);
  return (
    <div className="field">
      <span>
        Video helper
        <small>
          For YouTube videos without captions: BYTE gets the video's audio with yt-dlp (free and open source, {st ? bytes(st.approxBytes) : "about 36 MB"} {osText("from its official GitHub release, checked before use) and transcribes it on this Mac.")}
          {st?.version && ` Installed: ${st.version}.`}
        </small>
        {loading && (
          <span className="progress" aria-label={`Downloading ${pct([d])}%`} style={{ display: "block", marginTop: 6 }}>
            <span style={{ width: `${pct([d])}%` }} />
          </span>
        )}
        {error && <small className="bad">{error}</small>}
        {d?.phase === "failed" && <small className="bad">{d.error}</small>}
      </span>
      {loading ? (
        <span className="faint small">{d?.phase === "verifying" ? "Checking…" : `${pct([d])}%`}</span>
      ) : st?.version ? (
        <span className="row" style={{ gap: 6 }}>
          <button className="btn sm ghost" title="Get the newest yt-dlp (YouTube changes often; updating fixes most errors)" onClick={() => void api.mediaDownload().catch((e) => setError(errorText(e)))}>
            <RefreshCw size={13} /> Update
          </button>
          <button className="icon-btn sm" title="Delete the video helper" aria-label="Delete the video helper" onClick={() => void api.mediaDelete().then(refresh, (e) => setError(errorText(e)))}>
            <Trash2 size={13} />
          </button>
        </span>
      ) : (
        <button className="btn sm primary" onClick={() => void api.mediaDownload().catch((e) => setError(errorText(e)))}>
          <Download size={13} /> Download
        </button>
      )}
    </div>
  );
}

/** Settings → Models → Voice: reading answers aloud with the Mac's voices (speech.rs). */
export function SpeechRows() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [voices, setVoices] = useState<SpeechVoice[] | null>(null);
  const mac = canSpeak();
  useEffect(() => {
    if (mac) void api.speechVoices().then(setVoices, () => setVoices([]));
  }, [mac]);
  const lang = (navigator.language || "en-US").split("-")[0];
  const shown = (voices ?? []).filter((v) => v.language.startsWith(lang) || v.name === settings?.speechVoice);
  const speed = settings?.speechSpeed ?? "normal";
  return (
    <>
      <label className="field">
        <span>
          Read answers aloud
          <small>{mac ? osText("BYTE reads each answer aloud, with BYTE's voices when they're downloaded, otherwise the Mac's (the 🔊 on any answer does it once). Code and tables stay on screen.") : osText("Reading aloud needs a Mac for now.")}</small>
        </span>
        <input type="checkbox" disabled={!mac} checked={!!settings?.readAloud} onChange={(e) => void update({ readAloud: e.target.checked })} />
      </label>
      {mac && (
        <div className="field">
          <span>
            {osText("Speed, and the Mac's voice")}
            <small>{osText("The Mac's voice is used until BYTE's voices are downloaded. Better Mac voices (free): System Settings → Accessibility → Spoken Content → System voice → Manage Voices.")}</small>
          </span>
          <span className="row" style={{ gap: 6, flexWrap: "wrap", justifyContent: "flex-end" }}>
            <select value={settings?.speechVoice ?? ""} onChange={(e) => void update({ speechVoice: e.target.value })} aria-label="Voice">
              <option value="">{osText("Mac's default voice")}</option>
              {shown.map((v) => (
                <option key={v.name} value={v.name}>
                  {v.name}
                </option>
              ))}
            </select>
            <span className="segmented" role="group" aria-label="Speaking speed">
              {(["slow", "normal", "fast"] as const).map((s) => (
                <button key={s} aria-pressed={speed === s} onClick={() => void update({ speechSpeed: s })}>
                  {s[0].toUpperCase() + s.slice(1)}
                </button>
              ))}
            </span>
            <button className="btn sm ghost" onClick={() => void api.speechSay("Hi, I'm BYTE. This is how I'll sound when I read answers to you.")} title="Hear it">
              <Volume2 size={13} /> Try it
            </button>
          </span>
        </div>
      )}
    </>
  );
}

/** Settings → Models → Voice: "Hey BYTE" (wake.rs). */
export function WakeRow() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [ready, setReady] = useState<boolean | null>(null);
  useEffect(() => {
    if (inTauri) void api.wakeReady().then(setReady, () => setReady(false));
  }, []);
  const mac = canSpeak();
  return (
    <label className="field">
      <span>
        Listen for “Hey BYTE”
        <small>
          {!mac
            ? osText("“Hey BYTE” needs a Mac for now.")
            : ready === false
              ? "Download a speech model above first."
              : osText("Say “Hey BYTE, …” from anywhere and BYTE answers out loud, then listens a few seconds for a follow-up (say “thanks” to end). While this is on, the microphone stays on (macOS shows its orange dot); only short bursts of speech are checked, on this Mac, and nothing is kept. It pauses while BYTE talks or you record.")}
        </small>
        {mac && settings?.wakeWord && !settings.openAtLogin && (
          <small>
            To have it ready as soon as you log in:{" "}
            <button type="button" className="linklike" onClick={(e) => (e.preventDefault(), void update({ openAtLogin: true }))}>
              also open BYTE at login
            </button>{" "}
            {osText("(it starts hidden, in the menu bar).")}
          </small>
        )}
        {mac && settings?.wakeWord && settings.openAtLogin && <small>BYTE opens when you log in, so “Hey BYTE” works right away.</small>}
      </span>
      <input type="checkbox" disabled={!mac || ready === false} checked={!!settings?.wakeWord} onChange={(e) => void update({ wakeWord: e.target.checked })} />
    </label>
  );
}

const SAMPLE = "Hi, I'm BYTE. This is how I'll sound when I read my answers to you.";
const STYLES = [
  { id: "calm", label: "Calm", hint: "A little slower, with longer pauses" },
  { id: "natural", label: "Natural", hint: "Like a friendly conversation" },
  { id: "lively", label: "Lively", hint: "Brisker, with shorter pauses" },
];

/** Settings → Models → Voice: BYTE's voice (free voices made on this Mac, or BYTE Cloud's), its style, and the browser. */
export function ByteVoicesRow() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const cloud = useStore((s) => s.cloud);
  const [catalog, setCatalog] = useState<VoicePackage[]>([]);
  const [status, setStatus] = useState<VoicesStatus | null>(null);
  const [cloudVoices, setCloudVoices] = useState<CloudVoice[] | null>(null);
  const [freeRam, setFreeRam] = useState<number | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mac = canSpeak();
  const refresh = useCallback(() => {
    if (!inTauri) return;
    void api.voicesStatus().then((s) => {
      setStatus(s);
      useStore.setState({ voicesReady: s.ready.length > 0 });
    }, (e) => setError(errorText(e)));
  }, []);
  useEffect(() => {
    if (!inTauri) return;
    void api.voicesCatalog().then(setCatalog, (e) => setError(errorText(e)));
    void api.memoryReport().then((m) => setFreeRam(m.availableBytes), () => undefined);
    refresh();
  }, [refresh]);
  useEffect(() => {
    if (inTauri && cloud?.connected) void api.cloudVoices().then(setCloudVoices, () => setCloudVoices([]));
  }, [cloud?.connected]);
  if (!mac) return null;
  const current = pick(catalog, settings?.byteVoice);
  const ready = !!current && !!status?.ready.includes(current.pkg.id);
  const where = settings?.voiceWhere === "cloud" && cloud?.connected ? "cloud" : "mac";
  const lowRam = freeRam !== null && freeRam < 1.2e9 && !!current && current.pkg.ramMb > 200;
  return (
    <div className="field" style={{ display: "block" }}>
      <div className="row" style={{ alignItems: "flex-start", gap: 12 }}>
        <span className="grow">
          BYTE's voice
          <small>
            {ready && current
              ? // Only the fixed sentence is reworded, never the voice and provider names in front of it.
                `${current.speaker.name} · ${current.pkg.name}${current.pkg.name.startsWith(current.pkg.provider.split(" ")[0]) ? "" : ` (${current.pkg.provider})`}` +
                osText(". Natural voices made on this Mac, free and open source; BYTE starts speaking while an answer is still being written, without gaps.")
              : status?.ready.length
                ? "Pick a downloaded voice below."
                : osText("Natural, human-sounding voices instead of the Mac's robotic one: over 2,000 to choose from, free and open source, made on this Mac. Browse them and download the ones you like (from about 30 MB).")}
          </small>
          {lowRam && (
            <small className="warn">
              {osText("Your Mac is short on free memory right now. A lighter voice (Light on memory: Kitten nano or Piper, about 150 MB) leaves more room for the chat model.")}
            </small>
          )}
          {error && <small className="bad">{error}</small>}
        </span>
        <span className="row" style={{ gap: 6, flexWrap: "wrap", justifyContent: "flex-end" }}>
          {ready && (
            <button className="btn sm ghost" onClick={() => void api.speechSay(SAMPLE).catch((e) => setError(errorText(e)))} title="Hear BYTE's voice">
              <Volume2 size={13} /> Try it
            </button>
          )}
          <button className="btn sm primary" onClick={() => setOpen(!open)} aria-expanded={open}>
            {open ? "Close" : "Browse voices"}
          </button>
        </span>
      </div>
      <div className="row" style={{ gap: 10, marginTop: 8, flexWrap: "wrap" }}>
        <span className="faint">How BYTE speaks</span>
        <div className="segmented" role="group" aria-label="How BYTE speaks">
          {STYLES.map((st) => (
            <button key={st.id} title={st.hint} aria-pressed={(settings?.speechStyle ?? "natural") === st.id} onClick={() => void update({ speechStyle: st.id })}>
              {st.label}
            </button>
          ))}
        </div>
        {cloud?.connected && (
          <>
            <span className="faint">Made</span>
            <div className="segmented" role="group" aria-label="Where BYTE's voice is made">
              <button aria-pressed={where === "mac"} onClick={() => void update({ voiceWhere: "mac" })} title={osText("Free voices on this Mac: private, work offline")}>
                {osText("On this Mac")}
              </button>
              <button aria-pressed={where === "cloud"} onClick={() => void update({ voiceWhere: "cloud" })} title={osText("Voices on the BYTE cloud's GPU: no memory used on this Mac. Private chats always use this Mac.")}>
                BYTE Cloud
              </button>
            </div>
          </>
        )}
      </div>
      {where === "cloud" && (
        <div className="row" style={{ gap: 8, marginTop: 8 }}>
          {cloudVoices?.length ? (
            <select value={settings?.cloudVoice ?? ""} onChange={(e) => void update({ cloudVoice: e.target.value })} aria-label="BYTE Cloud voice">
              <option value="">The cloud's default voice</option>
              {cloudVoices.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.name}{v.expressive ? " ✨" : ""}{v.lang ? ` · ${v.lang}` : ""}
                </option>
              ))}
            </select>
          ) : (
            <small className="faint">{osText("Your BYTE Cloud doesn't offer voices yet, so BYTE speaks with the voice on this Mac. Private chats always do.")}</small>
          )}
        </div>
      )}
      {open && catalog.length > 0 && (
        <div style={{ marginTop: 10 }}>
          <VoiceBrowser catalog={catalog} status={status} onChanged={refresh} />
        </div>
      )}
    </div>
  );
}
