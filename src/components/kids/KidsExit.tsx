import { KeyRound, X } from "lucide-react";
import { useState } from "react";

import { api, errorText } from "../../lib/api";
import { useStore } from "../../state/store";

/** "Grown-ups": the PIN turns kids mode off (kids.rs checks it). */
export function KidsExit({ onClose }: { onClose: () => void }) {
  const [pin, setPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const settings = await api.kidsExit(pin);
      useStore.setState({ settings });
      onClose();
      await useStore.getState().reloadChats();
    } catch (e) {
      setError(errorText(e));
      setPin("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="writing-panel kids-exit" role="dialog" aria-modal="true" aria-label="Turn off kids mode">
        <div className="recipe-box-head">
          <KeyRound size={18} />
          <h2>For grown-ups</h2>
          <span className="grow" />
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>
        <p className="faint small">Type the PIN to turn off kids mode.</p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <input
            className="pin-input"
            type="password"
            inputMode="numeric"
            autoComplete="off"
            maxLength={6}
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/\D/g, ""))}
            aria-label="PIN"
            autoFocus
          />
          <button className="btn primary" type="submit" disabled={busy || pin.length < 4}>
            Turn off kids mode
          </button>
        </form>
        {error && <div className="banner danger">{error}</div>}
      </div>
    </div>
  );
}
