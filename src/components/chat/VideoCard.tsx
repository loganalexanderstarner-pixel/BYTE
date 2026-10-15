import { openUrl } from "@tauri-apps/plugin-opener";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { documentDir, join } from "@tauri-apps/api/path";
import { Check, Clapperboard, Copy, FileDown, Play } from "lucide-react";
import { useState } from "react";

import { api } from "../../lib/api";
import { DOC_THEMES, fileName } from "../../lib/docs/spec";
import { stamp, videoDocSpec, videoLink, videoText } from "../../lib/video";
import type { VideoCard as Video } from "../../lib/types";
import { osText } from "../../lib/platform";

/** A YouTube video's summary: thumbnail, TL;DR, key points and chapters, each opening the video at that moment. */
export function VideoCard({ video }: { video: Video }) {
  const [copied, setCopied] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const open = (secs: number) => void openUrl(videoLink(video.id, secs));
  const copy = () =>
    void navigator.clipboard?.writeText(videoText(video)).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  const pdf = async () => {
    try {
      const dest = await saveDialog({ defaultPath: await join(await documentDir(), "BYTE", fileName(video.title, "pdf")) });
      if (!dest) return;
      const { renderDoc } = await import("../../lib/docs/render");
      await api.docSave(dest, await renderDoc("pdf", videoDocSpec(video), DOC_THEMES[0], {}));
      setStatus("Saved the PDF");
    } catch (e) {
      setStatus(e instanceof Error ? e.message : String(e));
    }
  };
  return (
    <div className="video" role="region" aria-label={`Video: ${video.title}`}>
      <div className="video-top">
        <button className="video-thumb" onClick={() => open(0)} title="Watch on YouTube">
          <img src={video.thumbnail} alt="" loading="lazy" referrerPolicy="no-referrer" />
          <span className="play" aria-hidden>
            <Play size={20} />
          </span>
          <span className="len">{stamp(video.seconds)}</span>
        </button>
        <div className="video-info">
          <div className="video-kicker">
            <Clapperboard size={13} /> {video.channel}
          </div>
          <h3>{video.title}</h3>
          {video.tldr && <p className="video-tldr">{video.tldr}</p>}
          <div className="video-actions">
            <button className="btn sm ghost" onClick={copy}>
              {copied ? <Check size={13} /> : <Copy size={13} />} Copy summary
            </button>
            <button className="btn sm ghost" onClick={() => void pdf()}>
              <FileDown size={13} /> PDF
            </button>
            {video.transcribed ? (
              <span className="hint">{osText("No captions, so BYTE transcribed the audio on this Mac: names may be misheard")}</span>
            ) : (
              video.autoCaptions && <span className="hint">From auto-generated captions: names may be misheard</span>
            )}
            {status && <span className="hint ok">{status}</span>}
          </div>
        </div>
      </div>
      {video.keyPoints.length > 0 && (
        <ul className="video-points">
          {video.keyPoints.map((p) => (
            <li key={p.start + p.text}>
              <button className="ts" onClick={() => open(p.start)}>
                {stamp(p.start)}
              </button>
              <span>{p.text}</span>
            </li>
          ))}
        </ul>
      )}
      {video.chapters.length > 0 && (
        <ol className="video-chapters">
          {video.chapters.map((c) => (
            <li key={c.start}>
              <button className="ts" onClick={() => open(c.start)}>
                {stamp(c.start)}
              </button>
              <div>
                <b>{c.title}</b>
                {c.summary && <p>{c.summary}</p>}
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
