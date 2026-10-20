import { useEffect, useState } from "react";

import { api } from "../../lib/api";
import type { ImageProgress } from "../../lib/types";

export const PHASE: Record<ImageProgress["phase"], string> = {
  loading: "Loading the model…",
  drawing: "Drawing",
  finishing: "Finishing the picture…",
  enlarging: "Enlarging…",
  polishing: "Adding the finishing touches…",
};

/** The engine's progress while a picture is being made. */
export function useImageProgress(): ImageProgress | null {
  const [p, setP] = useState<ImageProgress | null>(null);
  useEffect(() => {
    let off: (() => void) | undefined;
    void api.onImageProgress(setP).then((u) => (off = u));
    return () => off?.();
  }, []);
  return p;
}

/** Two versions of a picture with a slider between them. */
export function BeforeAfter({ before, after }: { before: string; after: string }) {
  const [x, setX] = useState(50);
  return (
    <div>
      <div style={{ position: "relative", lineHeight: 0 }}>
        <img src={after} alt="After" style={{ width: "100%", borderRadius: 8, display: "block" }} />
        <img src={before} alt="Before" style={{ position: "absolute", inset: 0, width: "100%", height: "100%", borderRadius: 8, clipPath: `inset(0 ${100 - x}% 0 0)`, objectFit: "cover" }} />
        <div style={{ position: "absolute", top: 0, bottom: 0, left: `${x}%`, width: 2, background: "#fff", boxShadow: "0 0 4px rgba(0,0,0,0.6)", pointerEvents: "none" }} />
      </div>
      <input type="range" min={0} max={100} value={x} onChange={(e) => setX(Number(e.target.value))} style={{ width: "100%" }} aria-label="Before and after" />
      <div className="row faint small" style={{ justifyContent: "space-between" }}>
        <span>Before</span>
        <span>After</span>
      </div>
    </div>
  );
}
