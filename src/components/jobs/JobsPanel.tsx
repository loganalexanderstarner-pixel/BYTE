import { openUrl } from "@tauri-apps/plugin-opener";
import { Briefcase, ExternalLink, Link2, Loader2, MessageSquare, Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { STATUS_LABEL, STATUSES, blankJob, daysLeft, deadlineText, groupJobs } from "../../lib/jobs";
import type { Job } from "../../lib/types";
import { useStore } from "../../state/store";

/** One job, editable. */
function JobEditor({ job, onSave, onCancel }: { job: Job; onSave: (j: Job) => void; onCancel: () => void }) {
  const [j, setJ] = useState(job);
  const set = (k: keyof Job) => (e: { target: { value: string } }) => setJ({ ...j, [k]: e.target.value });
  return (
    <div className="job-editor">
      <div className="job-grid">
        <label>
          Company <input value={j.company} onChange={set("company")} />
        </label>
        <label>
          Role <input value={j.role} onChange={set("role")} />
        </label>
        <label>
          Location <input value={j.location} onChange={set("location")} />
        </label>
        <label>
          Pay <input value={j.pay} onChange={set("pay")} />
        </label>
        <label>
          Status
          <select value={j.status} onChange={set("status")}>
            {STATUSES.map((s) => (
              <option key={s} value={s}>
                {STATUS_LABEL[s]}
              </option>
            ))}
          </select>
        </label>
        <label>
          Apply by <input type="date" value={j.deadline} onChange={set("deadline")} />
        </label>
      </div>
      <label>
        Notes <textarea rows={3} value={j.notes} onChange={set("notes")} placeholder="Who you talked to, next steps, interview dates…" />
      </label>
      {j.summary && <p className="muted small">{j.summary}</p>}
      <div className="row" style={{ gap: 8, justifyContent: "flex-end" }}>
        <button className="btn sm" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn sm primary" onClick={() => onSave(j)}>
          Save
        </button>
      </div>
    </div>
  );
}

/** Job search tracker (💼): the jobs you're looking at, by status, soonest deadline first. */
export function JobsPanel({ onClose }: { onClose: () => void }) {
  const send = useStore((s) => s.send);
  const [jobs, setJobs] = useState<Job[] | null>(null);
  const [editing, setEditing] = useState<Job | null>(null);
  const [link, setLink] = useState("");
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    api.jobsList().then(setJobs, (e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);

  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const fromLink = () =>
    guard(async () => {
      setReading(true);
      try {
        setEditing(await api.jobFromUrl(link.trim()));
        setLink("");
      } finally {
        setReading(false);
      }
    });
  const save = (j: Job) =>
    guard(async () => {
      await api.jobSave(j);
      setEditing(null);
      load();
    });
  const prep = (j: Job) =>
    guard(async () => {
      const prompt = await api.jobPrepPrompt(j);
      onClose();
      await send(prompt);
    });

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="study-panel jobs-panel" role="dialog" aria-modal="true" aria-label="Jobs">
        <div className="recipe-box-head">
          <Briefcase size={18} />
          <h2>Job search</h2>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {error && <div className="banner danger">{error}</div>}
        {editing ? (
          <JobEditor job={editing} onSave={(j) => void save(j)} onCancel={() => setEditing(null)} />
        ) : (
          <>
            <div className="row job-add">
              <Link2 size={15} className="faint" />
              <input
                className="grow"
                value={link}
                onChange={(e) => setLink(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && link.trim() && void fromLink()}
                placeholder="Paste a job posting's link and BYTE fills it in"
              />
              <button className="btn sm" disabled={!link.trim().startsWith("http") || reading} onClick={() => void fromLink()}>
                {reading ? <Loader2 size={14} className="spin" /> : "Read"}
              </button>
              <button className="btn sm" onClick={() => setEditing(blankJob())} title="Add a job by hand">
                <Plus size={14} /> Add
              </button>
            </div>
            {jobs == null ? (
              <p className="muted">Loading…</p>
            ) : jobs.length === 0 ? (
              <div className="empty-box">
                <p>No jobs yet.</p>
                <p className="muted">Paste a posting's link above, or add one by hand. BYTE keeps track of deadlines and helps you prepare for interviews.</p>
              </div>
            ) : (
              groupJobs(jobs).map(([status, list]) => (
                <section key={status} className="job-group">
                  <h4>
                    {STATUS_LABEL[status]} <span className="faint">{list.length}</span>
                  </h4>
                  <ul className="decks">
                    {list.map((j) => {
                      const left = daysLeft(j.deadline);
                      return (
                        <li key={j.id}>
                          <button className="deck-info link-like" onClick={() => setEditing(j)} title="Edit">
                            <b>
                              {j.role || "Untitled role"} · {j.company}
                            </b>
                            <span className="muted small">
                              {[j.location, j.pay].filter(Boolean).join(" · ")}
                              {j.deadline && (
                                <span className={left != null && left <= 3 && left >= 0 ? "due" : ""}> · {deadlineText(j.deadline)}</span>
                              )}
                            </span>
                          </button>
                          {j.url && (
                            <button className="icon-btn" title="Open the posting" aria-label={`Open the posting for ${j.role}`} onClick={() => void openUrl(j.url)}>
                              <ExternalLink size={15} />
                            </button>
                          )}
                          <button className="icon-btn" title="Prepare for the interview (in the chat)" aria-label={`Prepare for ${j.role}`} onClick={() => void prep(j)}>
                            <MessageSquare size={15} />
                          </button>
                          <button
                            className="icon-btn"
                            title="Delete"
                            aria-label={`Delete ${j.role}`}
                            onClick={() => void guard(async () => (await api.jobDelete(j.id), load()))}
                          >
                            <Trash2 size={15} />
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                </section>
              ))
            )}
          </>
        )}
      </div>
    </div>
  );
}
