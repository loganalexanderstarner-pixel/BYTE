import { FileSpreadsheet, FileText, Globe, ImageIcon, Images, Loader2, Presentation, X } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../../lib/api";
import { idOf, isImage, listOf, titleOf } from "../../lib/cloudDocs";
import { useStore, type Attachment } from "../../state/store";
import type { LocalFile } from "../../lib/types";

/** Cloud images fetched once per session (library photos, document pages). */
const thumbs = new Map<string, Promise<string>>();

/** A cloud image as a data URL, from the session cache when it's been loaded
 * before. A failed load isn't cached, so it's tried again next time. */
export function cloudImageCached(path: string): Promise<string> {
  let p = thumbs.get(path);
  if (!p) {
    p = api.cloudImage(path).catch((e) => {
      thumbs.delete(path);
      throw e;
    });
    thumbs.set(path, p);
  }
  return p;
}

export function CloudThumb({ path, alt, className }: { path: string; alt: string; className?: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    cloudImageCached(path).then(
      (s) => alive && setSrc(s || null),
      () => alive && setSrc(null),
    );
    return () => {
      alive = false;
    };
  }, [path]);
  return src ? <img className={className} src={src} alt={alt} /> : <span className={`${className ?? ""} thumb-empty`} aria-label={alt} />;
}

/** Photos/files on a message, or waiting to be sent (with remove buttons). */
export function AttachmentChips({ items, onRemove }: { items: Attachment[]; onRemove?: (id: string) => void }) {
  if (!items.length) return null;
  return (
    <div className="attachments">
      {items.map((a) => (
        <span key={a.id} className="attachment" title={a.name}>
          {a.image ? <CloudThumb className="thumb" path={`/api/attachments/${a.id}/image`} alt={a.name} /> : <FileText size={14} />}
          <span className="name">{a.name}</span>
          {onRemove && (
            <button className="icon-btn" onClick={() => onRemove(a.id)} aria-label={`Remove ${a.name}`}>
              <X size={12} />
            </button>
          )}
        </span>
      ))}
    </div>
  );
}

/** Pages / slides / sheets, as a short label. */
export function fileDetail(f: LocalFile): string {
  const n = f.pages ?? 0;
  const unit = f.kind === "slides" ? "slide" : f.kind === "sheet" ? "sheet" : "page";
  const count = n > 0 ? `${n} ${unit}${n === 1 ? "" : "s"}` : "";
  const scan = f.ocr ? (f.kind === "image" ? "text read" : "scan: text read") : "";
  return [count, scan, f.truncated ? "long: best parts used" : ""].filter(Boolean).join(" · ");
}

/** Files read on this Mac (local chats), on a message or waiting to be sent. */
export function LocalFileChips({ files, onRemove }: { files: LocalFile[]; onRemove?: (index: number) => void }) {
  const openReader = useStore((s) => s.openReader);
  if (!files.length) return null;
  return (
    <div className="attachments">
      {files.map((f, i) => {
        const detail = fileDetail(f);
        // Sent files open in the reader (their text as BYTE read it).
        const readable = !onRemove && f.text.trim().length > 0;
        return (
          <span
            key={`${f.name}-${i}`}
            className={`attachment ${readable ? "clickable" : ""}`}
            title={detail ? `${f.name} (${detail})` : f.name}
            onClick={readable ? () => openReader({ title: f.name, text: f.text }) : undefined}
          >
            {f.image ? (
              <img className="thumb" src={f.image} alt={f.name} />
            ) : f.kind === "sheet" ? (
              <FileSpreadsheet size={14} />
            ) : f.kind === "slides" ? (
              <Presentation size={14} />
            ) : f.kind === "web" ? (
              <Globe size={14} />
            ) : (
              <FileText size={14} />
            )}
            <span className="name">{f.name}</span>
            {detail && <span className="faint">{detail}</span>}
            {onRemove && (
              <button className="icon-btn" onClick={() => onRemove(i)} aria-label={`Remove ${f.name}`}>
                <X size={12} />
              </button>
            )}
          </span>
        );
      })}
    </div>
  );
}

/** Pick photos already on the cloud instead of uploading them again. */
export function LibraryPicker({ onClose }: { onClose(): void }) {
  const attachExisting = useStore((s) => s.attachExisting);
  const [items, setItems] = useState<Attachment[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api
      .cloudGet("/api/attachments")
      .then((v) =>
        setItems(
          listOf(v)
            .map((r) => ({ id: idOf(r) ?? "", name: titleOf(r), image: isImage(r) }))
            .filter((a) => a.id),
        ),
      )
      .catch((e) => setError(String(e)));
  }, []);
  return (
    <div className="library-pop" role="dialog" aria-label="Your photos and files">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <b>
          <Images size={14} style={{ verticalAlign: -2 }} /> Your library
        </b>
        <button className="icon-btn" onClick={onClose} aria-label="Close library">
          <X size={14} />
        </button>
      </div>
      {error && <div className="faint">{error}</div>}
      {!items && !error && (
        <div className="faint">
          <Loader2 size={14} className="spin" /> Loading…
        </div>
      )}
      {items && items.length === 0 && <div className="faint">Nothing uploaded yet.</div>}
      <div className="library-grid">
        {items?.map((a) => (
          <button
            key={a.id}
            className="library-item"
            title={a.name}
            onClick={() => {
              attachExisting(a);
              onClose();
            }}
          >
            {a.image ? <CloudThumb className="thumb" path={`/api/attachments/${a.id}/image`} alt={a.name} /> : <ImageIcon size={18} />}
            <span className="name">{a.name}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
