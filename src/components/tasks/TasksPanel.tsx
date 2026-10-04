import { Bell, CalendarClock, Check, ListTodo, MessageSquare, Play, Plus, Sunrise, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText } from "../../lib/api";
import { dueText, fromInput, sortTasks, toInput } from "../../lib/tasks";
import type { Schedule, Task } from "../../lib/types";
import { useStore } from "../../state/store";
import { AutomationsSection } from "./AutomationsSection";
import { TrackersSection } from "./TrackersSection";
import { WatchSection } from "./WatchSection";

const REPEATS: { value: Task["repeat"]; label: string }[] = [
  { value: "", label: "Once" },
  { value: "daily", label: "Every day" },
  { value: "weekdays", label: "Weekdays" },
  { value: "weekly", label: "Every week" },
  { value: "monthly", label: "Every month" },
];

const blank = (): Task => ({ id: 0, title: "", notes: "", due: null, remindAt: null, repeat: "", doneAt: null, created: 0 });

/** ✅ BYTE's to-do list and the things it does on a schedule (the daily briefing, scheduled questions). */
export function TasksPanel({ onClose }: { onClose: () => void }) {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [draft, setDraft] = useState<Task>(blank);
  const [remind, setRemind] = useState(true);
  const [showDone, setShowDone] = useState(false);
  const [when, setWhen] = useState("");
  const [ask, setAsk] = useState("");
  const [parsed, setParsed] = useState<[string, string] | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const selectChat = useStore((s) => s.selectChat);
  const reloadChats = useStore((s) => s.reloadChats);
  const tasksOn = useStore((s) => s.settings?.tasksEnabled !== false);
  const watchOn = useStore((s) => s.settings?.watchEnabled !== false);
  const automationsOn = useStore((s) => s.settings?.automationsEnabled !== false);
  const trackersOn = useStore((s) => s.settings?.trackersEnabled !== false);

  const load = useCallback(() => {
    api.tasksList().then(setTasks, (e) => setError(errorText(e)));
    api.schedulesList().then(setSchedules, (e) => setError(errorText(e)));
    setNow(Date.now());
  }, []);
  useEffect(load, [load]);
  useEffect(() => {
    const t = setTimeout(() => {
      if (!when.trim()) return setParsed(null);
      api.scheduleParse(when).then(setParsed, () => setParsed(null));
    }, 200);
    return () => clearTimeout(t);
  }, [when]);

  const guard = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    load();
  };
  const add = () =>
    void guard(async () => {
      await api.taskSave({ ...draft, remindAt: remind ? draft.due : null });
      setDraft(blank());
    });
  const addSchedule = (kind: Schedule["kind"]) =>
    void guard(async () => {
      const spec = kind === "briefing" && !parsed ? "weekdays 07:30" : parsed?.[0];
      if (!spec) throw new Error("Say when, like “every weekday at 8am”.");
      const words = ask.trim().split(/\s+/);
      const name = kind === "briefing" ? "Morning briefing" : words.slice(0, 6).join(" ") + (words.length > 6 ? "…" : "");
      await api.scheduleSave({ id: 0, kind, name, spec, prompt: kind === "briefing" ? "" : ask.trim(), enabled: true, lastRun: null, nextRun: null, when: "", lastChat: null, lastOk: null });
      setWhen("");
      setAsk("");
    });
  const runNow = (s: Schedule) =>
    void guard(async () => {
      setBusy(s.id);
      try {
        const chat = await api.scheduleRun(s.id);
        if (chat) {
          await reloadChats();
          await selectChat(chat);
          onClose();
        }
      } finally {
        setBusy(null);
      }
    });
  const open = (chat: string) =>
    void guard(async () => {
      await reloadChats();
      await selectChat(chat);
      onClose();
    });

  const shown = tasks ? sortTasks(tasks).filter((t) => showDone || t.doneAt == null) : [];
  const doneCount = tasks?.filter((t) => t.doneAt != null).length ?? 0;

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="study-panel tasks-panel" role="dialog" aria-modal="true" aria-label="Tasks, schedules, trackers, automations, feeds and watched pages">
        <div className="recipe-box-head">
          <ListTodo size={18} />
          <h2>Tasks</h2>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {error && <div className="banner danger">{error}</div>}

        {tasksOn && (
          <>
            <form
              className="task-add"
              onSubmit={(e) => {
                e.preventDefault();
                if (draft.title.trim()) add();
              }}
            >
              <input className="grow" value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} placeholder="Add a to-do" aria-label="New to-do" />
              <input type="datetime-local" value={toInput(draft.due)} onChange={(e) => setDraft({ ...draft, due: fromInput(e.target.value) })} aria-label="Due" />
              {draft.due != null && (
                <>
                  <select value={draft.repeat} onChange={(e) => setDraft({ ...draft, repeat: e.target.value as Task["repeat"] })} aria-label="Repeat">
                    {REPEATS.map((r) => (
                      <option key={r.value} value={r.value}>
                        {r.label}
                      </option>
                    ))}
                  </select>
                  <label className="chip-toggle" title="A Mac notification when it's due">
                    <input type="checkbox" checked={remind} onChange={(e) => setRemind(e.target.checked)} />
                    <Bell size={13} /> Remind me
                  </label>
                </>
              )}
              <button className="btn sm primary" disabled={!draft.title.trim()}>
                <Plus size={14} /> Add
              </button>
            </form>

            {tasks == null ? (
              <p className="muted">Loading…</p>
            ) : shown.length === 0 ? (
              <p className="muted">Nothing to do. Add one above, or tell BYTE “add call the bank to my to-do list”.</p>
            ) : (
              <ul className="task-list">
                {shown.map((t) => {
                  const due = dueText(t.due, now);
                  return (
                    <li key={t.id} className={t.doneAt != null ? "done" : due.startsWith("Overdue") ? "overdue" : ""}>
                      <button
                        className={`check ${t.doneAt != null ? "on" : ""}`}
                        onClick={() => void guard(() => api.taskDone(t.id, t.doneAt == null))}
                        aria-label={t.doneAt != null ? `Mark ${t.title} not done` : `Mark ${t.title} done`}
                      >
                        {t.doneAt != null && <Check size={12} />}
                      </button>
                      <span className="title">{t.title}</span>
                      {due && t.doneAt == null && (
                        <span className="due small">
                          {t.remindAt != null && <Bell size={11} />} {due}
                          {t.repeat && ` · ${REPEATS.find((r) => r.value === t.repeat)?.label.toLowerCase()}`}
                        </span>
                      )}
                      <button className="icon-btn sm" onClick={() => void guard(() => api.taskDelete(t.id))} aria-label={`Delete ${t.title}`} title="Delete">
                        <Trash2 size={13} />
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
            {doneCount > 0 && (
              <button className="link small" onClick={() => setShowDone(!showDone)}>
                {showDone ? "Hide done" : `Show done (${doneCount})`}
              </button>
            )}

            <div className="recipe-box-head schedules-head">
              <CalendarClock size={16} />
              <h3>On a schedule</h3>
            </div>
            <p className="muted small">BYTE does these on its own while it's open (a missed one runs when you next open it). Each answer is saved as a chat, and you get a notification.</p>
            {schedules.length > 0 && (
              <ul className="schedule-list">
                {schedules.map((s) => (
                  <li key={s.id} className={s.enabled ? "" : "off"}>
                    {s.kind === "briefing" ? <Sunrise size={15} /> : <MessageSquare size={15} />}
                    <div className="grow">
                      <b>{s.name}</b>
                      <div className="muted small">
                        {s.when}
                        {s.enabled && s.nextRun != null && ` · next ${dueText(s.nextRun, now)}`}
                        {s.lastOk === false && <span className="hint danger"> · last run didn't work</span>}
                      </div>
                      {s.kind === "prompt" && <div className="small prompt">“{s.prompt}”</div>}
                    </div>
                    {s.lastChat && (
                      <button className="btn sm ghost" onClick={() => open(s.lastChat!)} title="Open the last answer">
                        Open last
                      </button>
                    )}
                    <button className="btn sm ghost" disabled={busy === s.id} onClick={() => runNow(s)} title="Run it now">
                      <Play size={13} /> {busy === s.id ? "Running…" : "Run now"}
                    </button>
                    <label className="switch" title={s.enabled ? "On" : "Off"}>
                      <input type="checkbox" checked={s.enabled} onChange={(e) => void guard(() => api.scheduleSave({ ...s, enabled: e.target.checked }))} aria-label={`${s.name} on`} />
                    </label>
                    <button className="icon-btn sm" onClick={() => void guard(() => api.scheduleDelete(s.id))} aria-label={`Delete ${s.name}`} title="Delete">
                      <Trash2 size={13} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <div className="schedule-add">
              <input value={when} onChange={(e) => setWhen(e.target.value)} placeholder="When: every weekday at 8am" aria-label="When" />
              <input className="grow" value={ask} onChange={(e) => setAsk(e.target.value)} placeholder="What should BYTE do? e.g. summarize the latest AI news" aria-label="What BYTE should do" />
              <button className="btn sm primary" disabled={!parsed || !ask.trim()} onClick={() => addSchedule("prompt")}>
                <Plus size={14} /> Add
              </button>
            </div>
            <div className="muted small schedule-hint">
              {when.trim() ? (parsed ? parsed[1] : "BYTE doesn't understand that time yet: try “every weekday at 8am” or “every monday at 9”.") : null}
              {!schedules.some((s) => s.kind === "briefing") && (
                <button className="btn sm ghost" onClick={() => addSchedule("briefing")} title="Calendar, reminders, to-dos, weather and news">
                  <Sunrise size={13} /> Add a daily briefing {parsed ? `(${parsed[1].toLowerCase()})` : "(weekdays 7:30 AM)"}
                </button>
              )}
            </div>
          </>
        )}
        {trackersOn && <TrackersSection />}
        {automationsOn && <AutomationsSection onClose={onClose} />}
        {watchOn && <WatchSection onClose={onClose} />}
      </div>
    </div>
  );
}
