import { useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { cleanList } from "../settings/ScreenSettings";
import { dayLabel, OWN_EDGE_WARNING, summaryApi, UNTIL_BACK_CHOICES, type DaySummary } from "../../lib/found";
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
      <DailySummary />
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
          <small>
            BYTE keeps researching while you are away, and stops when you say “I'm back” (typed, spoken, or the button on the live card), when you press Stop, or at the time limit (Unlimited has none). On Windows it opens a visible Edge window with a separate profile and reads each page in its own new tab, so you can watch them appear. To control that window, Edge listens on a debugging port on this computer only (127.0.0.1, a random number); the port exists only while a job runs and is closed when it ends. When it ends BYTE writes the summary and leaves the pages open in a normal Edge window with no port.
          </small>
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
          Close the tabs when I'm back
          <small>When the research ends, close the pages instead of leaving them open for you. The links are still saved in the chat. Off by default.</small>
        </span>
        <input type="checkbox" checked={settings?.browserCloseTabsWhenBack ?? false} onChange={(e) => void update({ browserCloseTabsWhenBack: e.target.checked })} aria-label="Close the tabs when I'm back" />
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

/** Settings → Control and screen → Never touch: the one list used by PC control and by the screen features. */
export function NeverTouchList() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setText((settings?.screenExcludeApps ?? []).join("\n"));
  }, [settings?.screenExcludeApps]);
  const guard = (p: Promise<unknown>) => p.then(() => setError(null), (e) => setError(errorText(e)));
  return (
    <div className="section" id="never-touch">
      <h4>Never touch</h4>
      <div className="field" style={{ display: "block" }}>
        <span>
          Programs and windows BYTE leaves alone
          <small>One per line: part of a program's name or of a window's title (any capitals). PC control will not click, type or open anything there, and the screen features will not look at it. The starting list holds password managers, banking and private browser windows. BYTE's own window is always left alone.</small>
        </span>
        <textarea
          aria-label="Programs and windows BYTE never touches"
          rows={6}
          style={{ width: "100%", marginTop: 6 }}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={() => void guard(update({ screenExcludeApps: cleanList(text) }))}
        />
        <button className="btn sm" style={{ marginTop: 6 }} onClick={() => void api.screenDefaultExcludes().then((d) => guard(update({ screenExcludeApps: d })))}>
          Reset to the starting list
        </button>
        {error && <small className="bad">{error}</small>}
      </div>
    </div>
  );
}

/** Settings → General: the daily "what BYTE did" summary (off by default), with the last days listed when it is on. */
function DailySummary() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [days, setDays] = useState<DaySummary[]>([]);
  const on = settings?.dailySummary ?? false;
  useEffect(() => {
    if (on) void summaryApi.days(7).then(setDays, () => setDays([]));
  }, [on]);
  return (
    <>
      <label className="field">
        <span>
          Daily “what BYTE did” summary
          <small>A short note in your morning briefing and here: how many PC steps, web searches and pages BYTE did, and when. Only the kind of thing and the time, never what was searched, typed or opened. Off until you turn it on.</small>
        </span>
        <input type="checkbox" checked={on} onChange={(e) => void update({ dailySummary: e.target.checked })} aria-label="Daily what BYTE did summary" />
      </label>
      {on && (
        <div role="region" aria-label="What BYTE did in the last days" style={{ marginBottom: 8 }}>
          {days.filter((d) => d.lines.length).length === 0 && <small>Nothing yet.</small>}
          {days
            .filter((d) => d.lines.length)
            .map((d) => (
              <div key={d.day}>
                <b>{dayLabel(d.day)}</b>
                <ul>
                  {d.lines.map((l) => (
                    <li key={l}>{l}</li>
                  ))}
                </ul>
              </div>
            ))}
        </div>
      )}
    </>
  );
}
