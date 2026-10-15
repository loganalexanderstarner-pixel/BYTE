import { Fingerprint, Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Logo } from "../design/Logo";
import { api, errorText, inTauri } from "../lib/api";
import { osText } from "../lib/platform";

/**
 * Keeps track of the lock (lock.rs): whether BYTE is locked now, and tells
 * Rust the user is active (at most every 30 s) so the idle timer restarts.
 */
export function useLock(): boolean | null {
  // null until Rust has said (nothing is shown in between, so no chats flash by).
  const [locked, setLocked] = useState<boolean | null>(inTauri ? null : false);
  const last = useRef(0);

  useEffect(() => {
    if (!inTauri) return;
    void api.lockStatus().then((s) => setLocked(s.locked), () => setLocked(false));
    const off = api.onLockChanged(setLocked);
    const touch = () => {
      const now = Date.now();
      if (now - last.current > 30_000) {
        last.current = now;
        void api.lockTouch().catch(() => undefined);
      }
    };
    window.addEventListener("pointerdown", touch);
    window.addEventListener("keydown", touch);
    return () => {
      void off.then((f) => f());
      window.removeEventListener("pointerdown", touch);
      window.removeEventListener("keydown", touch);
    };
  }, []);
  return locked;
}

/** The whole window while BYTE is locked: Touch ID or the Mac's password (Windows Hello on a PC) opens it. */
export function LockScreen({ compact = false }: { compact?: boolean }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The platform's own wording comes from Rust; these are the Mac words, used
  // until it answers and by any older backend that does not send them.
  const [method, setMethod] = useState(osText("Touch ID"));
  const [hint, setHint] = useState(osText("No Touch ID? macOS asks for your password instead."));
  useEffect(() => {
    if (!inTauri) return;
    void api.lockStatus().then(
      (s) => {
        if (s.method) setMethod(s.method);
        if (s.hint) setHint(s.hint);
      },
      () => undefined,
    );
  }, []);

  const unlock = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await api.lockUnlock();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }, []);

  // Ask right away when the window shows (not in the small Quick Ask window, where a click is expected).
  useEffect(() => {
    if (!compact) void unlock();
  }, [compact, unlock]);

  return (
    <div className={`lock-screen ${compact ? "compact" : ""}`} data-tauri-drag-region>
      <div className="lock-card">
        <Logo size={compact ? 40 : 64} />
        <h2>BYTE is locked</h2>
        <p className="faint">Your chats, notes and memories stay hidden until you unlock.</p>
        <button className="btn primary" onClick={() => void unlock()} disabled={busy} autoFocus>
          {busy ? <Loader2 size={16} className="spin" /> : <Fingerprint size={16} />} Unlock with {method}
        </button>
        <p className="faint small">{hint}</p>
        {error && <p className="lock-error small">{error}</p>}
      </div>
    </div>
  );
}
