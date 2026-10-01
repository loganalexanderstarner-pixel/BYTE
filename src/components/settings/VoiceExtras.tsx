import { Download, RefreshCw, Trash2, Volume2 } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import { bytes } from "../../lib/format";
import type { MediaStatus, SpeakersStatus, SpeechVoice } from "../../lib/types";
import { canSpeak, useStore, type DownloadState } from "../../state/store";

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
          For YouTube videos without captions: BYTE gets the video's audio with yt-dlp (free and open source, {st ? bytes(st.approxBytes) : "about 36 MB"} from its official GitHub release, checked before use) and transcribes it on this Mac.
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
          <small>{mac ? "BYTE reads each answer with your Mac's own voice (the 🔊 on any answer does it once). Code and tables stay on screen." : "Reading aloud needs a Mac for now."}</small>
        </span>
        <input type="checkbox" disabled={!mac} checked={!!settings?.readAloud} onChange={(e) => void update({ readAloud: e.target.checked })} />
      </label>
      {mac && (
        <div className="field">
          <span>
            Voice and speed
            <small>More voices: System Settings → Accessibility → Spoken Content → System voice → Manage Voices.</small>
          </span>
          <span className="row" style={{ gap: 6, flexWrap: "wrap", justifyContent: "flex-end" }}>
            <select value={settings?.speechVoice ?? ""} onChange={(e) => void update({ speechVoice: e.target.value })} aria-label="Voice">
              <option value="">Mac's default voice</option>
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
            ? "“Hey BYTE” needs a Mac for now."
            : ready === false
              ? "Download a speech model above first."
              : "Say “Hey BYTE” and Quick Ask opens, listening. While this is on, the microphone stays on (macOS shows its orange dot); only short bursts of speech are checked, on this Mac, and nothing is kept. It pauses while BYTE talks or you record."}
        </small>
      </span>
      <input type="checkbox" disabled={!mac || ready === false} checked={!!settings?.wakeWord} onChange={(e) => void update({ wakeWord: e.target.checked })} />
    </label>
  );
}
