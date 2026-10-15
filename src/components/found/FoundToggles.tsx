import { useStore } from "../../state/store";

/** Settings → General: the foundations track's on/off switches. Every one starts off. */
export function FoundToggles() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  return (
    <div className="section">
      <h4>Laptop and activity</h4>
      <label className="field">
        <span>
          Laptop mode
          <small>While your computer runs on its battery, BYTE pauses listening for “Hey BYTE” and the screen replay, shows a small badge, and offers a smaller downloaded model. Your settings are not changed: everything comes back on mains power. Off until you turn it on.</small>
        </span>
        <input type="checkbox" checked={settings?.laptopMode ?? false} onChange={(e) => void update({ laptopMode: e.target.checked })} aria-label="Laptop mode" />
      </label>
      <label className="field">
        <span>
          Approval popup
          <small>When BYTE needs a yes or no and its window is not in front, a small popup appears on top with Do it and Don't. It does not take the keyboard from what you are typing; press Ctrl+Alt+Y or Ctrl+Alt+N to answer from anywhere. It counts as “no” when the time runs out, and nothing is ever approved by itself.</small>
        </span>
        <input type="checkbox" checked={settings?.approvalPopup ?? true} onChange={(e) => void update({ approvalPopup: e.target.checked })} aria-label="Approval popup" />
      </label>
      <label className="field">
        <span>
          Speak and listen for approvals
          <small>BYTE also says what it wants to do and listens for a spoken “yes” or “no”, the same way it listens after “Hey BYTE” (so Hey BYTE must be on). Off until you turn it on.</small>
        </span>
        <input type="checkbox" checked={settings?.approvalVoice ?? false} disabled={!settings?.wakeWord} onChange={(e) => void update({ approvalVoice: e.target.checked })} aria-label="Speak and listen for approvals" />
      </label>
    </div>
  );
}
