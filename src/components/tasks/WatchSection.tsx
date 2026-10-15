import { Eye, Newspaper, Plus, RefreshCw, Rss, Tag, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import type { Feed, Watcher } from "../../lib/types";
import { CHECK_EVERY, ago, money, parseTarget, watchSummary } from "../../lib/watch";
import { useStore } from "../../state/store";

const blankWatcher = (): Watcher => ({
  id: 0,
  url: "",
  name: "",
  kind: "change",
  target: null,
  everyHours: 6,
  enabled: true,
  created: 0,
  lastChecked: null,
  nextCheck: null,
  lastPrice: null,
  currency: "",
  lastChange: null,
  lastNote: "",
  lastError: "",
});

/** News feeds and watched pages, in the ✅ panel (Rust `feeds.rs`, `watchers.rs`). */
export function WatchSection({ onClose }: { onClose: () => void }) {
  const [feeds, setFeeds] = useState<Feed[]>([]);
  const [watchers, setWatchers] = useState<Watcher[]>([]);
  const [feedUrl, setFeedUrl] = useState("");
  const [draft, setDraft] = useState<Watcher>(blankWatcher);
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const webOn = useStore((s) => s.settings?.webSearch !== false);
  const newChat = useStore((s) => s.newChat);
  const send = useStore((s) => s.send);

  const load = useCallback(() => {
    api.feedsList().then(setFeeds, (e) => setError(errorText(e)));
    api.watchersList().then(setWatchers, (e) => setError(errorText(e)));
    setNow(Date.now());
  }, []);
  useEffect(load, [load]);
  useEffect(() => {
    const off = api.onWatcherChanged(() => load());
    return () => void off.then((f) => f());
  }, [load]);

  const guard = async (key: string, fn: () => Promise<unknown>) => {
    setError(null);
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
    load();
  };

  const follow = () =>
    void guard("follow", async () => {
      await api.feedFollow(feedUrl.trim());
      setFeedUrl("");
    });
  const digest = () => {
    newChat();
    onClose();
    void send("What's new in my feeds?");
  };
  const watch = () =>
    void guard("watch", async () => {
      const url = /^https?:\/\//i.test(draft.url.trim()) ? draft.url.trim() : `https://${draft.url.trim()}`;
      const saved = await api.watcherSave({
        ...draft,
        url,
        target: draft.kind === "price" ? parseTarget(target) : null,
      });
      // The first look sets what later checks compare with.
      await api.watcherCheck(saved.id);
      setDraft(blankWatcher());
      setTarget("");
    });

  const unseen = feeds.reduce((n, f) => n + f.unseen, 0);

  return (
    <>
      {error && <div className="banner danger">{error}</div>}
      {!webOn && <div className="banner">Feeds and watched pages need web access, which is off in Settings.</div>}

      <div className="recipe-box-head schedules-head">
        <Rss size={16} />
        <h3>News feeds</h3>
        <span className="spacer" />
        {feeds.length > 0 && (
          <button className="btn sm ghost" onClick={digest} disabled={!webOn} title="A digest of what's new, with sources">
            <Newspaper size={13} /> What's new{unseen > 0 ? ` (${unseen})` : ""}
          </button>
        )}
      </div>
      <p className="muted small">
        Follow a site and BYTE catches you up on its new stories: “what's new in my feeds?”. For a digest every morning, add a schedule such as “every day at 7am” → “what's new in my feeds”.
      </p>
      {feeds.length > 0 && (
        <ul className="schedule-list">
          {feeds.map((f) => (
            <li key={f.id}>
              <Rss size={15} />
              <div className="grow">
                <b>{f.title}</b>
                {f.unseen > 0 && <span className="feed-new">{f.unseen} new</span>}
                <div className="muted small watch-url">{f.url}</div>
                {f.lastError && <div className="hint danger small">Couldn't read it last time: {f.lastError}</div>}
              </div>
              <button className="icon-btn sm" onClick={() => void guard(`feed-${f.id}`, () => api.feedDelete(f.id))} aria-label={`Unfollow ${f.title}`} title="Unfollow">
                <Trash2 size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="schedule-add"
        onSubmit={(e) => {
          e.preventDefault();
          if (feedUrl.trim()) follow();
        }}
      >
        <input className="grow" value={feedUrl} onChange={(e) => setFeedUrl(e.target.value)} placeholder="A site or feed address, e.g. theverge.com" aria-label="Site or feed address" />
        <button className="btn sm primary" disabled={!feedUrl.trim() || !webOn || busy === "follow"}>
          <Plus size={14} /> {busy === "follow" ? "Finding the feed…" : "Follow"}
        </button>
      </form>

      <div className="recipe-box-head schedules-head">
        <Eye size={16} />
        <h3>Watched pages</h3>
      </div>
      <p className="muted small">BYTE checks these while it's open and sends a notification when the page changes or the price drops. Prices come from the store page's own product data.</p>
      {watchers.length > 0 && (
        <ul className="schedule-list">
          {watchers.map((w) => (
            <li key={w.id} className={w.enabled ? "" : "off"}>
              {w.kind === "price" ? <Tag size={15} /> : <Eye size={15} />}
              <div className="grow">
                <b>{w.name}</b>
                {w.lastPrice != null && <span className="feed-new">{money(w.lastPrice, w.currency)}</span>}
                <div className="muted small">
                  {watchSummary(w)}
                  {w.lastChecked != null && ` · checked ${ago(w.lastChecked, now)}`}
                </div>
                {w.lastNote && (
                  <div className="small prompt">
                    {w.lastNote}
                    {w.lastChange != null && <span className="muted"> · {ago(w.lastChange, now)}</span>}
                  </div>
                )}
                {w.lastError && <div className="hint danger small">{w.lastError}</div>}
              </div>
              <a className="btn sm ghost" href={w.url} target="_blank" rel="noreferrer" title={w.url}>
                Open
              </a>
              <button className="btn sm ghost" disabled={busy === `check-${w.id}` || !webOn} onClick={() => void guard(`check-${w.id}`, () => api.watcherCheck(w.id))} title="Check now">
                <RefreshCw size={13} className={busy === `check-${w.id}` ? "spin" : ""} />
              </button>
              <label className="switch" title={w.enabled ? "On" : "Off"}>
                <input type="checkbox" checked={w.enabled} onChange={(e) => void guard(`on-${w.id}`, () => api.watcherSave({ ...w, enabled: e.target.checked }))} aria-label={`${w.name} on`} />
              </label>
              <button className="icon-btn sm" onClick={() => void guard(`del-${w.id}`, () => api.watcherDelete(w.id))} aria-label={`Stop watching ${w.name}`} title="Stop watching">
                <Trash2 size={13} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="schedule-add"
        onSubmit={(e) => {
          e.preventDefault();
          if (draft.url.trim()) watch();
        }}
      >
        <input className="grow" value={draft.url} onChange={(e) => setDraft({ ...draft, url: e.target.value })} placeholder="Page to watch (a link)" aria-label="Page to watch" />
        <select value={draft.kind} onChange={(e) => setDraft({ ...draft, kind: e.target.value as Watcher["kind"] })} aria-label="Watch for">
          <option value="change">Any change</option>
          <option value="price">Price drop</option>
        </select>
        {draft.kind === "price" && (
          <input className="target-input" value={target} onChange={(e) => setTarget(e.target.value)} placeholder="Target (optional)" aria-label="Target price" inputMode="decimal" />
        )}
        <select value={draft.everyHours} onChange={(e) => setDraft({ ...draft, everyHours: Number(e.target.value) })} aria-label="How often">
          {CHECK_EVERY.map((c) => (
            <option key={c.hours} value={c.hours}>
              {c.label}
            </option>
          ))}
        </select>
        <button className="btn sm primary" disabled={!draft.url.trim() || !webOn || busy === "watch"}>
          <Plus size={14} /> {busy === "watch" ? "Reading…" : "Watch"}
        </button>
      </form>
    </>
  );
}
