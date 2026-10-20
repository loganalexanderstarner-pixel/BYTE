import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { took } from "../../lib/imageSizes";
import { bytes } from "../../lib/format";
import type { ImageGalleryItem } from "../../lib/types";
import { useStore } from "../../state/store";

const KIND: Record<string, string> = { made: "Made", edited: "Edited", enlarged: "Enlarged", poster: "Poster", changed: "Changed", "": "Picture" };

function when(sec: number): string {
  return sec ? new Date(sec * 1000).toLocaleString() : "";
}

/** The pictures BYTE made or edited, newest first. */
export function Gallery({ onEdit }: { onEdit?: (path: string) => void }) {
  const [items, setItems] = useState<ImageGalleryItem[] | null>(null);
  const [open, setOpen] = useState<ImageGalleryItem | null>(null);
  const [big, setBig] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const keep = useStore((s) => s.settings?.imageKeep ?? "all");
  const reload = useCallback(() => {
    api.imageGallery().then(setItems).catch((e) => (setItems([]), setError(errorText(e))));
  }, []);
  useEffect(reload, [reload]);
  useEffect(() => {
    setBig("");
    if (open) void api.imagePreview(open.path, 1600).then((s) => setBig(s.preview)).catch(() => setBig(open.thumb));
  }, [open]);

  const run = (fn: () => Promise<unknown>) => fn().then(reload).catch((e) => setError(errorText(e)));

  return (
    <div className="section">
      <h4>Gallery</h4>
      {keep !== "all" && <p className="faint small">{keep === "saved" ? "Only pictures you save are kept. Others are removed when you make the next one." : "BYTE keeps nothing after showing it. Change this under “Keep my pictures”."}</p>}
      {error && <div className="banner danger" role="alert">{error}</div>}
      {items === null ? (
        <p className="faint">Loading…</p>
      ) : items.length === 0 ? (
        <p className="faint">No pictures yet. Make one under “Make”.</p>
      ) : (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(150px, 1fr))", gap: 10 }}>
          {items.map((it) => (
            <button key={it.path} className="model-card" style={{ padding: 6, textAlign: "left" }} onClick={() => setOpen(it)} aria-label={`${KIND[it.kind] ?? "Picture"}: ${it.prompt || "picture"}`}>
              <img src={it.thumb} alt="" style={{ width: "100%", aspectRatio: "1", objectFit: "cover", borderRadius: 6 }} />
              <div className="small" style={{ marginTop: 4, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{it.prompt || KIND[it.kind]}</div>
              <div className="faint small">{KIND[it.kind] ?? "Picture"}{it.saved && keep === "saved" ? " · saved" : ""}</div>
            </button>
          ))}
        </div>
      )}
      {open && (
        <div role="dialog" aria-modal="true" aria-label="Picture" className="modal-backdrop" onClick={() => setOpen(null)} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.7)", display: "grid", placeItems: "center", zIndex: 50, padding: 16 }}>
          <div className="model-card" style={{ maxWidth: 900, maxHeight: "92vh", overflow: "auto", background: "var(--surface, #1b1b1f)" }} onClick={(e) => e.stopPropagation()}>
            <img src={big || open.thumb} alt={open.prompt} style={{ maxWidth: "100%", maxHeight: "60vh", borderRadius: 8, display: "block", margin: "0 auto" }} />
            <p className="small" style={{ marginTop: 8 }}>{open.prompt}</p>
            <p className="faint small">
              {KIND[open.kind] ?? "Picture"}{open.model ? ` · ${open.model}` : ""} · {open.width} × {open.height} · {bytes(open.bytes)}{open.seconds ? ` · ${took(open.seconds)}` : ""}{open.seed >= 0 && open.kind === "made" ? ` · seed ${open.seed}` : ""}
              {open.notes.length ? ` · ${open.notes.join(" · ")}` : ""} · {when(open.created)}
            </p>
            <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
              <button className="btn" onClick={() => void run(() => api.imageGallerySave(open.path, !open.saved).then(() => setOpen({ ...open, saved: !open.saved })))}>{open.saved ? "Unsave" : "Save"}</button>
              <button
                className="btn"
                onClick={() =>
                  void saveDialog({ defaultPath: open.path.split(/[\\/]/).pop(), filters: [{ name: "Picture", extensions: ["png", "jpg"] }] })
                    .then((dest) => (dest ? api.imageCopyTo(open.path, dest) : undefined))
                    .catch((e) => setError(errorText(e)))
                }
              >
                Save a copy…
              </button>
              {onEdit && <button className="btn" onClick={() => (onEdit(open.path), setOpen(null))}>Edit this</button>}
              <button className="btn danger" onClick={() => void run(() => api.imageGalleryDelete(open.path).then(() => setOpen(null)))}>Delete</button>
              <button className="btn" onClick={() => setOpen(null)}>Close</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** A small gallery to pick one picture from. */
export function PicturePicker({ onPick, onClose }: { onPick: (path: string) => void; onClose: () => void }) {
  const [items, setItems] = useState<ImageGalleryItem[] | null>(null);
  useEffect(() => {
    api.imageGallery().then(setItems).catch(() => setItems([]));
  }, []);
  return (
    <div className="model-card" style={{ marginBottom: 8 }}>
      <div className="row" style={{ gap: 8 }}>
        <b className="grow">Pick a picture</b>
        <button className="btn sm" onClick={onClose}>Close</button>
      </div>
      {items === null ? (
        <p className="faint">Loading…</p>
      ) : items.length === 0 ? (
        <p className="faint">Your gallery is empty. Make a picture first.</p>
      ) : (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(110px, 1fr))", gap: 8, maxHeight: 280, overflow: "auto" }}>
          {items.map((it) => (
            <button key={it.path} className="model-card" style={{ padding: 4 }} onClick={() => onPick(it.path)} aria-label={it.prompt || "picture"}>
              <img src={it.thumb} alt="" style={{ width: "100%", aspectRatio: "1", objectFit: "cover", borderRadius: 4 }} />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
