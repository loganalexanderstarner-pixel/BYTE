import { useCallback, useEffect, useState } from "react";

import { api } from "../../lib/api";
import { addSite, clampSteps, removeSite } from "../../lib/browserSites";
import type { BrowserActivity } from "../../lib/types";
import { useStore } from "../../state/store";

const DEFAULT_STEPS = 25;

const REAL_WARNING =
  "BYTE will start Microsoft Edge or Google Chrome and control it for you: it opens pages, clicks and types in that window.\n\n" +
  "It uses a separate profile folder that BYTE makes, never your own logins, bookmarks or history. " +
  "It still asks you before it submits, buys, sends, deletes or downloads anything, and it never types passwords or card numbers.\n\n" +
  "Anything you sign in to inside that window stays in BYTE's profile folder. Turn this on?";

/** Settings, Browser: what BYTE may do in a web browser, which sites, and what it did. */
export function BrowserSettings() {
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const [blocked, setBlocked] = useState<[string, string[]][]>([]);
  const [log, setLog] = useState<BrowserActivity[]>([]);
  const [allowText, setAllowText] = useState("");
  const [blockText, setBlockText] = useState("");
  const [open, setOpen] = useState(false);

  const reload = useCallback(() => {
    api.browserActivity(100).then(setLog).catch(() => setLog([]));
  }, []);
  useEffect(() => {
    if (!open) return;
    api.browserBuiltinBlocked().then(setBlocked).catch(() => setBlocked([]));
    reload();
  }, [open, reload]);

  if (!settings) return null;
  const sites = settings.browserSites ?? { allow: [], block: [] };
  const setSites = (next: { allow: string[]; block: string[] }) => void update({ browserSites: next });
  const steps = settings.browserMaxSteps ? settings.browserMaxSteps : DEFAULT_STEPS;

  const list = (kind: "allow" | "block", text: string, setText: (s: string) => void) => (
    <div className="field">
      <span>
        {kind === "allow" ? "Only these sites" : "Never these sites"}
        <small>
          {kind === "allow"
            ? "Leave empty to allow any public site. With entries, BYTE opens only these (and their sub-sites)."
            : "Always wins, even over the allowed list."}
        </small>
        <span style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 6 }}>
          {sites[kind].map((s) => (
            <button key={s} onClick={() => setSites({ ...sites, [kind]: removeSite(sites[kind], s) })} title="Remove">
              {s} ×
            </button>
          ))}
        </span>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            setSites({ ...sites, [kind]: addSite(sites[kind], text) });
            setText("");
          }}
        >
          <input value={text} onChange={(e) => setText(e.target.value)} placeholder="example.com" aria-label={kind === "allow" ? "Add an allowed site" : "Add a blocked site"} />
          <button type="submit">Add</button>
        </form>
      </span>
    </div>
  );

  return (
    <div className="browser-settings">
      <label className="field">
        <span>
          Watch BYTE browse
          <small>Shows BYTE's private browser window while it works, with a “BYTE is browsing” bar and a Stop button. Off: the window stays hidden.</small>
        </span>
        <input type="checkbox" checked={settings.browserVisible ?? false} onChange={(e) => void update({ browserVisible: e.target.checked })} />
      </label>
      <label className="field">
        <span>
          Live research
          <small>“Research …” and “look up the latest on …” open several sources one after another and answer with the sources named. Asking “in the browser” always does this.</small>
        </span>
        <input type="checkbox" checked={settings.browserLiveResearch ?? false} onChange={(e) => void update({ browserLiveResearch: e.target.checked })} />
      </label>
      <label className="field">
        <span>
          Use Edge or Chrome
          <small>BYTE drives Microsoft Edge or Google Chrome instead of its built-in window, with a separate profile of its own. Not used in private chats.</small>
        </span>
        <input
          type="checkbox"
          checked={settings.browserDriveReal ?? false}
          onChange={(e) => {
            if (e.target.checked && !window.confirm(REAL_WARNING)) return;
            void update({ browserDriveReal: e.target.checked });
          }}
        />
      </label>
      <label className="field">
        <span>
          Most steps per task
          <small>Pages opened, clicks and typing in one answer. Default {DEFAULT_STEPS}, at most 100.</small>
        </span>
        <input
          type="number"
          min={1}
          max={100}
          value={steps}
          onChange={(e) => void update({ browserMaxSteps: clampSteps(Number(e.target.value)) })}
          aria-label="Most browser steps per task"
        />
      </label>
      {list("allow", allowText, setAllowText)}
      {list("block", blockText, setBlockText)}
      <details onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
        <summary>What BYTE can and cannot do in the browser, and what it did</summary>
        <p>
          BYTE can open public web pages, read them, click links and buttons, fill in ordinary text boxes and choose from lists. It asks you first before anything is submitted,
          bought, booked, sent or deleted, and before every download. It never types passwords, card numbers or other private details, never opens files or your local network, and
          closes the browser when the answer ends. Text on a page is treated as information, not as instructions.
        </p>
        <p>
          Blocked by default: banks and payments, password managers, health portals, government sites and big sign-in pages.
          {blocked.length > 0 && (
            <>
              {" "}
              {blocked.map(([cat, sites2]) => (
                <span key={cat} style={{ display: "block" }}>
                  <b>{cat}:</b> {sites2.join(", ")}
                </span>
              ))}
            </>
          )}
          Adding one of them to “Only these sites” opens it (and limits BYTE to that list).
        </p>
        <p>
          <b>Activity</b> (addresses and kinds of step; typed text is never kept; stored only on this PC){" "}
          <button
            onClick={() => {
              void api.browserActivityClear().then(reload);
            }}
          >
            Clear
          </button>
        </p>
        {log.length === 0 ? (
          <p>Nothing yet.</p>
        ) : (
          <ul>
            {log.map((a, i) => (
              <li key={`${a.at}-${i}`}>
                {new Date(a.at).toLocaleString()} · {a.action} · {a.url}
              </li>
            ))}
          </ul>
        )}
      </details>
    </div>
  );
}
