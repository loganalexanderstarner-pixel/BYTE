import { OWN_EDGE_WARNING, UNTIL_BACK_CHOICES } from "../../lib/found";
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

/** Settings → Control and screen → Browser: research that leaves its pages behind. Every one starts off. */
export function FoundBrowserToggles() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  return (
    <div className="section">
      <h4>Browser research</h4>
      <label className="field">
        <span>
          Keep the pages BYTE opened for me
          <small>With “Watch BYTE browse” on, BYTE's browsing window stays open after the task so you can read the pages, and the list of pages is added to the chat as links. Later you can say “summarise what you read” and BYTE opens those links again. Not used in private chats.</small>
        </span>
        <input type="checkbox" checked={settings?.browserKeepPages ?? false} onChange={(e) => void update({ browserKeepPages: e.target.checked })} aria-label="Keep the pages BYTE opened for me" />
      </label>
      <label className="field">
        <span>
          Keep researching until I'm back
          <small>After the first answer BYTE goes on with more searches and pages from new angles while you are away, and stops when you are back, at the time limit, or when you press Stop. Choose Unlimited for no time limit.</small>
        </span>
        <select
          value={settings?.browserResearchUntilBack ?? "off"}
          aria-label="Keep researching until I'm back"
          onChange={(e) => void update({ browserResearchUntilBack: e.target.value as NonNullable<typeof settings>["browserResearchUntilBack"] })}
        >
          {UNTIL_BACK_CHOICES.map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>
          Use my own Edge profile
          <small>Strong warning: BYTE would use your everyday Edge with your sign-ins instead of its own separate profile. Off by default; you are asked to confirm.</small>
        </span>
        <input
          type="checkbox"
          checked={settings?.browserOwnEdge ?? false}
          aria-label="Use my own Edge profile"
          onChange={(e) => {
            if (e.target.checked && !window.confirm(OWN_EDGE_WARNING)) return;
            void update({ browserOwnEdge: e.target.checked });
          }}
        />
      </label>
    </div>
  );
}
