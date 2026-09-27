import { FileText, ImageIcon, Images, Loader2, X } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../../lib/api";
import { idOf, isImage, listOf, titleOf } from "../../lib/cloudDocs";
import { useStore, type Attachment } from "../../state/store";

/** Thumbnails fetched once per session (the library can hold many photos). */
const thumbs = new Map<string, Promise<string>>();

export function CloudThumb({ path, alt, className }: { path: string; alt: string; className?: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    if (!thumbs.has(path)) thumbs.set(path, api.cloudImage(path).catch(() => ""));
    void thumbs.get(path)!.then((s) => alive && setSrc(s || null));
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
