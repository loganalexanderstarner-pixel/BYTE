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
    </div>
  );
}
