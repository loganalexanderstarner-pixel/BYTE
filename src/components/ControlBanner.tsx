import { listen } from "@tauri-apps/api/event";
import { Square } from "lucide-react";
import { useEffect, useState } from "react";

import { inTauri } from "../lib/api";
import { bannerText, controlApi, type ControlState } from "../lib/control";

/** A bar that shows while BYTE is operating the PC, with a Stop button. Follows the `control://state` event. */
export function ControlBanner() {
  const [state, setState] = useState<ControlState>({ active: false, goal: "" });
  useEffect(() => {
    if (!inTauri) return;
    const off = listen<ControlState>("control://state", (e) => setState(e.payload));
    return () => void off.then((f) => f());
  }, []);
  if (!state.active) return null;
  return (
    <div
      role="status"
      aria-live="polite"
      style={{
        position: "fixed",
        left: 12,
        right: 12,
        bottom: 12,
        zIndex: 60,
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "8px 12px",
        borderRadius: 10,
        background: "var(--panel)",
        color: "var(--text)",
        border: "1px solid var(--accent)",
      }}
    >
      <span className="grow">{bannerText(state.goal)}</span>
      <button className="btn sm primary" onClick={() => void controlApi.stop()}>
        <Square size={14} /> Stop
      </button>
    </div>
  );
}
