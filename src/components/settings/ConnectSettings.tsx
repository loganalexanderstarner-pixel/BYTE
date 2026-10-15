import { openUrl } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Check, FolderOpen, Play, Plug, RotateCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText, inTauri } from "../../lib/api";
import {
  blankJob, countLabel, defaultWhen, EMAIL_STEPS, groupChannels, jobProblem, KEEP, keepLabel, PER_DAY, PER_HOUR, PER_MINUTE, TOOL_GROUPS, toggleChannel, TRIGGER_LABEL, WHEN_CHOICES, wasSkipped, WIZARDS,
  type AutoJob, type ChannelInfo, type ConnectorKind, type ConnectorPatch, type ConnectorRow, type ConnectStatus, type JobRun, type When,
} from "../../lib/connect";

const when = (ms: number | null) => (ms ? new Date(ms).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }) : "never");

function Select({ value, options, label, text, onChange }: { value: number; options: number[]; label: string; text: (n: number) => string; onChange: (n: number) => void }) {
  const all = options.includes(value) ? options : [...options, value];
  return (
    <select aria-label={label} value={value} onChange={(e) => onChange(Number(e.target.value))}>
      {all.map((n) => (
        <option key={n} value={n}>
          {text(n)}
        </option>
      ))}
    </select>
  );
}

function Steps({ steps }: { steps: { text: string; link?: { label: string; url: string } }[] }) {
  return (
    <ol className="connect-steps">
      {steps.map((s, i) => (
        <li key={i}>
          {s.text}{" "}
          {s.link && (
            <button className="link" onClick={() => void openUrl(s.link!.url)}>
              {s.link.label}
            </button>
          )}
        </li>
      ))}
    </ol>
  );
}

/** One connector: numbered setup, token check, channel picker, then the switches. */
function ConnectorCard({ row, reload, setError }: { row: ConnectorRow; reload: (s: ConnectStatus) => void; setError: (e: string | null) => void }) {
  const kind = row.kind;
  const email = kind === "email";
  const webhook = kind === "discord-webhook";
  const wiz = email ? null : WIZARDS[kind as Exclude<ConnectorKind, "email">];
  const [open, setOpen] = useState(false);
  const [secret, setSecret] = useState("");
  const [host, setHost] = useState(row.host);
  const [user, setUser] = useState(row.user);
  const [folder, setFolder] = useState(row.folder || "INBOX");
  const [drafts, setDrafts] = useState(row.draftsFolder || "Drafts");
  const [who, setWho] = useState<string | null>(null);
  const [found, setFound] = useState<ChannelInfo[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const run = async (fn: () => Promise<void>) => {
    setError(null);
    setNote(null);
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const patch = (p: ConnectorPatch) => run(async () => reload(await api.connectSet(kind, p)));
  const check = () =>
    run(async () => {
      const name = email ? await api.connectEmailSet(host, user, secret, folder, drafts) : await api.connectTokenSet(kind, secret);
      setSecret("");
      setWho(name);
      reload(await api.connectStatus());
    });
  const find = () => run(async () => setFound(await api.connectChannels(kind)));
  const tick = (c: ChannelInfo) => patch({ channels: toggleChannel(row.channels, c) });
  const allowed = new Set(row.channels.map((c) => c.id));
  const listed = found ?? row.channels.map((c) => ({ ...c, group: "Allowed" }));

  return (
    <div className="section connector">
      <h4>
        <Plug size={15} /> {wiz?.title ?? "Email (read, draft replies)"}{" "}
        <span className={`connect-badge ${row.enabled ? "on" : ""}`}>{row.enabled ? "On" : row.hasToken ? "Set up, off" : "Not set up"}</span>
      </h4>
      <button className="btn sm ghost" onClick={() => setOpen(!open)} aria-expanded={open}>
        {open ? "Hide setup steps" : row.hasToken ? "Change setup" : "Set up"}
      </button>
      {open && (
        <>
          <Steps steps={wiz?.steps ?? EMAIL_STEPS} />
          {wiz?.note && <p className="muted small">{wiz.note}</p>}
          {email && (
            <div className="connect-form">
              <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="Server, e.g. imap.gmail.com" aria-label="Mail server" />
              <input value={user} onChange={(e) => setUser(e.target.value)} placeholder="Your address" aria-label="Your address" autoComplete="off" />
              <input value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="Folder to read" aria-label="Folder to read" />
              <input value={drafts} onChange={(e) => setDrafts(e.target.value)} placeholder="Drafts folder" aria-label="Drafts folder" />
            </div>
          )}
          <div className="connect-form">
            <input type="password" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder={email ? "App password" : wiz?.secretLabel} aria-label={email ? "App password" : wiz?.secretLabel} autoComplete="off" spellCheck={false} />
            <button className="btn sm primary" disabled={busy || !secret.trim()} onClick={() => void check()}>
              <Check size={13} /> Check and save
            </button>
          </div>
          <p className="faint small">Kept in the system's credential store, never in a file or a log. Nothing is shared with anyone else.</p>
        </>
      )}
      {who && <p className="small">Connected: <b>{who}</b></p>}
      {note && <p className="small">{note}</p>}

      {row.hasToken && !webhook && (
        <>
          <p className="small">
            <b>Allowed {email ? "folders" : "channels"}</b>: BYTE uses only the ones ticked.{" "}
            <button className="btn sm" disabled={busy} onClick={() => void find()}>
              {kind === "telegram" ? "Find chats" : email ? "Find folders" : "Find channels"}
            </button>
          </p>
          {listed.length === 0 && <p className="faint small">Nothing yet. Press the button to look.</p>}
          {groupChannels(listed).map((g) => (
            <div key={g.group} className="connect-group">
              <span className="faint small">{g.group}</span>
              {g.channels.map((c) => (
                <label key={c.id} className="row" style={{ gap: 8 }}>
                  <input type="checkbox" checked={allowed.has(c.id)} disabled={busy} onChange={() => void tick(c)} />
                  <span>{c.name}</span>
                </label>
              ))}
            </div>
          ))}
        </>
      )}

      {row.hasToken && (
        <>
          <label className="row" style={{ gap: 10 }}>
            <input type="checkbox" checked={row.enabled} disabled={busy} onChange={(e) => void patch({ enabled: e.target.checked })} />
            <span>
              <b>Turn {row.name} on</b>
              <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
                Off: BYTE doesn't read or post anything here. {webhook ? "On: BYTE may post to the webhook's channel when you or a job asks." : "On: it reads only the ticked channels and posts only to them."}
              </span>
            </span>
          </label>
          <div className="connect-limits">
            {!webhook && (
              <label>
                Reads <Select value={row.readsPerMin} options={PER_MINUTE} label="Reads per minute" text={(n) => countLabel(n, "minute")} onChange={(n) => void patch({ readsPerMin: n })} />
              </label>
            )}
            <label>
              Posts <Select value={row.postsPerHour} options={PER_HOUR} label="Posts per hour" text={(n) => countLabel(n, "hour")} onChange={(n) => void patch({ postsPerHour: n })} />
            </label>
          </div>
          {!webhook && (
            <label className="row" style={{ gap: 10 }}>
              <input type="checkbox" checked={row.inDigest} disabled={busy} onChange={(e) => void patch({ inDigest: e.target.checked })} />
              <span>Include these channels in the morning digest</span>
            </label>
          )}
          {!webhook && (
            <>
              <label className="row" style={{ gap: 10 }}>
                <input type="checkbox" checked={row.replyForMe} disabled={busy} onChange={(e) => void patch({ replyForMe: e.target.checked })} />
                <span>
                  <b>Reply for me</b>
                  <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
                    BYTE writes a reply to each new message and shows it to you. {email ? "Nothing is saved as a draft until you say yes, and BYTE never sends email itself." : "Nothing is posted until you say yes."}
                  </span>
                </span>
              </label>
              {row.replyForMe && !email && (
                <label className="small">
                  Auto-reply without asking, in one channel only:{" "}
                  <select value={row.autoReplyChannel ?? ""} onChange={(e) => void patch({ autoReplyChannel: e.target.value || null })} aria-label="Auto-reply channel">
                    <option value="">None (always ask)</option>
                    {row.channels.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
                  </select>
                  {row.autoReplyChannel && <Select value={row.autoReplyPerHour} options={[2, 6, 20, 0]} label="Auto-replies per hour" text={(n) => countLabel(n, "hour")} onChange={(n) => void patch({ autoReplyPerHour: n })} />}
                </label>
              )}
            </>
          )}
          {kind === "telegram" && (
            <div className="connect-phone">
              <label className="row" style={{ gap: 10 }}>
                <input type="checkbox" checked={row.phoneAsk} disabled={busy} onChange={(e) => void patch({ phoneAsk: e.target.checked })} />
                <span>
                  <b>Ask the computer from the phone</b>
                  <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
                    A message from your private chat can run one of your saved routines ("run game time") or ask a question; the answer comes back to the chat.
                  </span>
                </span>
              </label>
              {row.phoneAsk && (
                <>
                  <label className="row" style={{ gap: 10 }}>
                    <input type="checkbox" checked={row.confirmEach} disabled={busy} onChange={(e) => void patch({ confirmEach: e.target.checked })} />
                    <span>Ask me on the computer before each one</span>
                  </label>
                  <label className="row" style={{ gap: 10 }}>
                    <input type="checkbox" checked={row.allowControl} disabled={busy} onChange={(e) => void patch({ allowControl: e.target.checked })} />
                    <span>
                      <b>Allow control from Telegram</b>
                      <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
                        Off: only questions and saved routines. On: a message may also operate this PC (needs PC control to be on, and it follows its approval rules).
                      </span>
                    </span>
                  </label>
                </>
              )}
            </div>
          )}
          {row.enabled && row.channels.length > 0 && !email && (
            <button className="btn sm" disabled={busy} onClick={() => void run(async () => { await api.connectPostTest(kind, row.channels[0].id); setNote("A test message was sent."); })}>
              Send a test message
            </button>
          )}
          {row.enabled && webhook && (
            <button className="btn sm" disabled={busy} onClick={() => void run(async () => { await api.connectPostTest(kind, "webhook"); setNote("A test message was sent."); })}>
              Send a test message
            </button>
          )}{" "}
          <button className="btn sm ghost" disabled={busy} onClick={() => void run(async () => { reload(await api.connectForget(kind)); setWho(null); setFound(null); })}>
            <Trash2 size={13} /> Remove token and settings
          </button>
        </>
      )}
    </div>
  );
}

function Waiting({ status, reload, setError }: { status: ConnectStatus; reload: () => void; setError: (e: string | null) => void }) {
  const [edits, setEdits] = useState<Record<string, string>>({});
  if (status.pending.length === 0) return null;
  const go = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    reload();
  };
  return (
    <div className="section connector">
      <h4>Waiting for your yes ({status.pending.length})</h4>
      {status.pending.map((p) => (
        <div key={p.id} className="connect-pending">
          <p className="small faint">
            {p.kind === "phone" ? "Request from your phone" : `${p.author} in ${p.channelLabel}`}: “{p.incoming}”
          </p>
          <textarea rows={3} value={edits[p.id] ?? p.draft} onChange={(e) => setEdits({ ...edits, [p.id]: e.target.value })} readOnly={p.kind === "phone"} aria-label="The reply" />
          <div className="row" style={{ gap: 8 }}>
            <button className="btn sm primary" onClick={() => void go(() => api.connectPendingYes(p.id, p.kind === "phone" ? null : (edits[p.id] ?? null)))}>
              {p.kind === "phone" ? "Yes, do it" : p.connector === "email" ? "Yes, save as a draft" : "Yes, send"}
            </button>
            {p.connector === "email" && (
              <button className="btn sm" onClick={() => void go(async () => openUrl(await api.connectMailto(p.id, edits[p.id] ?? null)))}>
                Open in my mail app
              </button>
            )}
            <button className="btn sm ghost" onClick={() => void go(() => api.connectPendingNo(p.id))}>
              No
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}

function Activity() {
  const [log, setLog] = useState<Awaited<ReturnType<typeof api.connectLog>> | null>(null);
  return (
    <div className="section connector">
      <h4>What connectors did</h4>
      <p className="faint small">Times, channels and counts only. BYTE never writes message text here.</p>
      <button className="btn sm" onClick={() => void api.connectLog(40).then(setLog, () => setLog([]))}>
        Show recent activity
      </button>{" "}
      {log && log.length > 0 && (
        <button className="btn sm ghost" onClick={() => void api.connectLogClear().then(() => setLog([]))}>
          Clear
        </button>
      )}
      {log && log.length === 0 && <p className="faint small">Nothing yet.</p>}
      {log && log.length > 0 && (
        <ul className="connect-log">
          {log.map((e, i) => (
            <li key={i} className={e.ok ? "" : "bad"}>
              {when(e.at)} · {e.connector} · {e.action}
              {e.channel ? ` · ${e.channel}` : ""}
              {e.count ? ` · ${e.count}` : ""}
              {e.note ? ` · ${e.note}` : ""}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------- jobs

function JobForm({ job, rows, onSave, onCancel }: { job: AutoJob; rows: ConnectorRow[]; onSave: (j: AutoJob) => void; onCancel: () => void }) {
  const [j, setJ] = useState(job);
  const problem = jobProblem(j);
  const w = j.when;
  const set = (p: Partial<AutoJob>) => setJ({ ...j, ...p });
  const setWhen = (x: When) => set({ when: x });
  const digest = j.template === "digest";
  return (
    <div className="section connector">
      <h4>{job.id ? "Edit job" : "New job"}</h4>
      <input value={j.name} onChange={(e) => set({ name: e.target.value })} placeholder="Name, e.g. Morning news" aria-label="Job name" />
      {!digest && <textarea rows={3} value={j.instruction} onChange={(e) => set({ instruction: e.target.value })} placeholder="What should BYTE do? e.g. Summarise today's news about electric cars in five lines." aria-label="What to do" />}
      <label className="small">
        When:{" "}
        <select value={w.type} onChange={(e) => setWhen(defaultWhen(e.target.value as When["type"]))} aria-label="When">
          {WHEN_CHOICES.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
            </option>
          ))}
        </select>{" "}
        {(w.type === "daily" || w.type === "weekdays") && <input type="time" value={w.time} onChange={(e) => setWhen({ ...w, time: e.target.value })} aria-label="Time" />}
        {w.type === "everyHours" && <input type="number" min={1} max={168} value={w.hours} onChange={(e) => setWhen({ ...w, hours: Number(e.target.value) })} aria-label="Hours" style={{ width: 70 }} />}
        {w.type === "folder" && (
          <>
            <input value={w.path} onChange={(e) => setWhen({ ...w, path: e.target.value })} placeholder="Folder" aria-label="Folder" />{" "}
            <button className="btn sm" onClick={() => void openDialog({ directory: true }).then((d) => typeof d === "string" && setWhen({ ...w, path: d }))}>
              <FolderOpen size={13} /> Choose…
            </button>
          </>
        )}
        {w.type === "message" && (
          <>
            <select value={w.connector} onChange={(e) => setWhen({ ...w, connector: e.target.value, channel: "" })} aria-label="Connector">
              {rows.filter((r) => r.kind !== "discord-webhook").map((r) => (
                <option key={r.kind} value={r.kind}>
                  {r.name}
                </option>
              ))}
            </select>{" "}
            <select value={w.channel} onChange={(e) => setWhen({ ...w, channel: e.target.value })} aria-label="Channel">
              <option value="">Any allowed channel</option>
              {(rows.find((r) => r.kind === w.connector)?.channels ?? []).map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </>
        )}
        {w.type === "watcher" && <input type="number" min={1} value={w.id || ""} onChange={(e) => setWhen({ ...w, id: Number(e.target.value) })} placeholder="Watcher number" aria-label="Watcher number" style={{ width: 130 }} />}
      </label>
      {!digest && (
        <fieldset className="connect-tools">
          <legend className="small">What the job may use</legend>
          {TOOL_GROUPS.map((g) => (
            <label key={g.id} className="row" style={{ gap: 8 }} title={g.hint}>
              <input type="checkbox" checked={j.tools.includes(g.id)} onChange={(e) => set({ tools: e.target.checked ? [...j.tools, g.id] : j.tools.filter((t) => t !== g.id) })} />
              <span>{g.label}</span>
            </label>
          ))}
          <label className="row" style={{ gap: 8 }}>
            <input type="checkbox" checked={j.needsPc} onChange={(e) => set({ needsPc: e.target.checked })} />
            <span>It operates this PC (runs only while PC control is on, with its usual approvals)</span>
          </label>
        </fieldset>
      )}
      <label className="row" style={{ gap: 8 }}>
        <input type="checkbox" checked={j.speak} onChange={(e) => set({ speak: e.target.checked })} />
        <span>Say the result out loud</span>
      </label>
      <label className="small">
        Also post the result to:{" "}
        <select
          value={j.postTo ? `${j.postTo.connector}|${j.postTo.channel}` : ""}
          onChange={(e) => {
            const [connector, channel] = e.target.value.split("|");
            set({ postTo: e.target.value ? { connector, channel } : null });
          }}
          aria-label="Post the result to"
        >
          <option value="">Nowhere</option>
          {rows.filter((r) => r.enabled && r.kind !== "email").flatMap((r) => (r.kind === "discord-webhook" ? [{ k: `${r.kind}|webhook`, t: `${r.name}` }] : r.channels.map((c) => ({ k: `${r.kind}|${c.id}`, t: `${r.name} ${c.name}` })))).map((o) => (
            <option key={o.k} value={o.k}>
              {o.t}
            </option>
          ))}
        </select>
      </label>
      <div className="connect-limits">
        <label>
          Runs <Select value={j.maxRunsPerDay} options={PER_DAY} label="Runs per day" text={(n) => countLabel(n, "day")} onChange={(n) => set({ maxRunsPerDay: n })} />
        </label>
        <label>
          History <Select value={j.keepRuns} options={KEEP} label="History kept" text={keepLabel} onChange={(n) => set({ keepRuns: n })} />
        </label>
      </div>
      {problem && <p className="faint small">{problem}</p>}
      <div className="row" style={{ gap: 8 }}>
        <button className="btn sm primary" disabled={!!problem} onClick={() => onSave(j)}>
          Save
        </button>
        <button className="btn sm ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

function JobsSection({ status, reload, setError }: { status: ConnectStatus; reload: (s?: ConnectStatus) => void; setError: (e: string | null) => void }) {
  const [jobs, setJobs] = useState<AutoJob[]>([]);
  const [editing, setEditing] = useState<AutoJob | null>(null);
  const [history, setHistory] = useState<{ job: AutoJob; runs: JobRun[] } | null>(null);
  const load = useCallback(() => void api.autojobsList().then(setJobs, (e) => setError(errorText(e))), [setError]);
  useEffect(() => {
    load();
    let un: (() => void) | undefined;
    void api.onJobRan(() => load()).then((u) => (un = u));
    return () => un?.();
  }, [load]);
  const act = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    load();
  };
  const showHistory = async (job: AutoJob) => setHistory({ job, runs: await api.autojobRuns(job.id, 0) });

  return (
    <>
      <h3>Jobs and triggers</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        Save an instruction and let BYTE run it on a schedule or when something happens. Every job starts off. Jobs run with nobody watching: anything that needs an approval is not done.
      </p>
      <div className="section connector">
        <label className="row" style={{ gap: 10 }}>
          <input type="checkbox" checked={status.jobsPaused} onChange={(e) => void act(async () => reload(await api.jobsPause(e.target.checked)))} />
          <span>
            <b>Pause all jobs</b>
            <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
              One switch for every schedule and trigger. You can still press Run on a job yourself.
            </span>
          </span>
        </label>
      </div>
      {editing ? (
        <JobForm job={editing} rows={status.rows} onCancel={() => setEditing(null)} onSave={(j) => void act(async () => { await api.autojobSave(j); setEditing(null); })} />
      ) : (
        <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
          <button className="btn sm primary" onClick={() => setEditing(blankJob())}>
            New job
          </button>
          {!jobs.some((j) => j.template === "digest") && (
            <button className="btn sm" onClick={() => void act(() => api.autojobDigestTemplate("07:30", true))} title="The briefing, your news feeds and the channels you allowed for it, read aloud">
              Add “read my digest aloud”
            </button>
          )}
        </div>
      )}
      {jobs.length === 0 && !editing && <p className="faint small">No jobs yet.</p>}
      {jobs.map((j) => (
        <div key={j.id} className="section connector">
          <label className="row" style={{ gap: 10 }}>
            <input type="checkbox" checked={j.enabled} onChange={(e) => void act(() => api.autojobToggle(j.id, e.target.checked))} aria-label={`Turn ${j.name} on`} />
            <span>
              <b>{j.name}</b> {j.template === "digest" && <span className="connect-badge">digest</span>} {j.needsPc && <span className="connect-badge">PC</span>}
              <span className="faint" style={{ display: "block", fontSize: "0.88em" }}>
                {j.describe} · last run {when(j.lastRun)}
                {j.lastOk === false ? " (didn't finish)" : ""}
                {j.enabled && j.nextRun ? ` · next ${when(j.nextRun)}` : ""}
              </span>
            </span>
          </label>
          <div className="row" style={{ gap: 6, flexWrap: "wrap" }}>
            <button className="btn sm" onClick={() => void act(() => api.autojobRun(j.id))}>
              <Play size={13} /> Run now
            </button>
            <button className="btn sm" onClick={() => void showHistory(j)}>
              <RotateCw size={13} /> History
            </button>
            <button className="btn sm ghost" onClick={() => setEditing(j)}>
              Edit
            </button>
            <button className="btn sm ghost" onClick={() => void act(() => api.autojobDelete(j.id))}>
              <Trash2 size={13} /> Delete
            </button>
          </div>
          {history?.job.id === j.id && (
            <ul className="connect-log">
              {history.runs.length === 0 && <li className="faint">It has not run yet.</li>}
              {history.runs.map((r) => (
                <li key={r.id} className={r.ok === false && !wasSkipped(r) ? "bad" : ""}>
                  {when(r.started)} · {TRIGGER_LABEL[r.trigger] ?? r.trigger} · {r.summary || (r.finished ? "done" : "running…")}{" "}
                  <button className="link" onClick={() => void act(() => api.autojobRun(j.id))}>
                    Rerun
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      ))}
    </>
  );
}

/** Settings → Control and screen: connectors (Discord, Telegram, Slack, email) and jobs. */
export function ConnectSettings() {
  const [status, setStatus] = useState<ConnectStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback((s?: ConnectStatus) => {
    if (s) return setStatus(s);
    api.connectStatus().then(setStatus, (e) => setError(errorText(e)));
  }, []);
  useEffect(() => {
    if (!inTauri) return;
    load();
    let un: (() => void) | undefined;
    void api.onConnectPending(() => load()).then((u) => (un = u));
    return () => un?.();
  }, [load]);
  if (!inTauri) return null;
  return (
    <>
      <h3>Chat apps and email</h3>
      <p className="muted" style={{ marginTop: 0 }}>
        Connect your own Discord, Telegram or Slack bot, or your mail, so BYTE can read the channels you pick and post for you. Each is off until you set it up and turn it on, uses only your own token (kept in the credential store), and treats everything it reads as plain information, never as instructions.
      </p>
      {error && <div className="banner danger">{error}</div>}
      {status && <Waiting status={status} reload={() => load()} setError={setError} />}
      {status?.rows.map((r) => (
        <ConnectorCard key={r.kind} row={r} reload={(s) => load(s)} setError={setError} />
      ))}
      <Activity />
      {status && <JobsSection status={status} reload={(s) => load(s)} setError={setError} />}
    </>
  );
}
