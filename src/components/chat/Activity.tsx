import { openUrl } from "@tauri-apps/plugin-opener";
import { Calculator, ChevronRight, CircleCheck, CloudSun, FolderSearch, Globe, LoaderCircle, Search, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { fileSource } from "../../lib/reader";
import type { Source } from "../../lib/types";
import { useStore, type Step } from "../../state/store";

function hostOf(url: string): string {
  if (url.startsWith("file:")) return "your files";
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

function stepLabel(s: Step): { icon: typeof Search; text: string } {
  const arg = (k: string) => String(s.args?.[k] ?? "");
  switch (s.name) {
    case "web_search":
      return { icon: Search, text: `Searched “${arg("query")}”` };
    case "read_page":
      return { icon: Globe, text: `Read ${hostOf(arg("url"))}` };
    case "calculate":
      return { icon: Calculator, text: `Calculated ${arg("expression")}` };
    case "weather":
      return { icon: CloudSun, text: `Checked the forecast for ${arg("place")}` };
    case "search_my_files":
      return { icon: FolderSearch, text: `Searched your files for “${arg("query")}”` };
    default:
      return { icon: CircleCheck, text: s.name };
  }
}

/** Summary line like "Searched the web twice · read 3 pages". */
export function activitySummary(steps: Step[]): string {
  const searches = steps.filter((s) => s.name === "web_search").length;
  const reads = steps.filter((s) => s.name === "read_page" && s.status === "ok").length;
  const calcs = steps.filter((s) => s.name === "calculate").length;
  const weather = steps.some((s) => s.name === "weather" && s.status === "ok");
  const files = steps.some((s) => s.name === "search_my_files");
  const parts: string[] = [];
  if (files) parts.push("Searched your files");
  if (weather) parts.push("Checked the forecast");
  if (searches) parts.push(searches === 1 ? "Searched the web" : `Searched the web ${searches} times`);
  if (reads) parts.push(`read ${reads} page${reads === 1 ? "" : "s"}`);
  if (calcs) parts.push(`${calcs} calculation${calcs === 1 ? "" : "s"}`);
  const s = parts.join(" · ");
  return s ? s[0].toUpperCase() + s.slice(1) : "Used tools";
}

/** Live list of what BYTE is doing: searches, pages read, calculations. */
export function Activity({ steps, live }: { steps: Step[]; live: boolean }) {
  const [open, setOpen] = useState(false);
  const running = steps.find((s) => s.status === "running");
  const expanded = open || live;
  return (
    <div className="activity">
      <button className="activity-head" onClick={() => setOpen(!open)} aria-expanded={expanded}>
        {running ? <LoaderCircle size={14} className="spin" /> : <CircleCheck size={14} />}
        <span>{running ? `${stepLabel(running).text}…` : activitySummary(steps)}</span>
        <ChevronRight size={14} className="chev" style={{ transform: expanded ? "rotate(90deg)" : undefined }} />
      </button>
      {expanded && (
        <ol className="activity-list">
          {steps.map((s) => {
            const { icon: Icon, text } = stepLabel(s);
            return (
              <li key={s.id} className={s.status}>
                {s.status === "running" ? (
                  <LoaderCircle size={13} className="spin" />
                ) : s.status === "error" ? (
                  <TriangleAlert size={13} />
                ) : (
                  <Icon size={13} />
                )}
                <span className="what">{text}</span>
                {s.summary && <span className="result">{s.summary}</span>}
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}

/** Numbered source cards shown under an answer. Read pages come first. */
export function Sources({ sources }: { sources: Source[] }) {
  const [all, setAll] = useState(false);
  const openReader = useStore((s) => s.openReader);
  // Passages from the user's files open in the reader; web pages in the browser.
  const open = (s: Source) => {
    const f = fileSource(s.url);
    if (f) openReader({ title: s.title, path: f.path, page: f.page, highlight: s.snippet });
    else void openUrl(s.url);
  };
  const sorted = [...sources].sort((a, b) => Number(b.read) - Number(a.read) || a.n - b.n);
  const shown = all ? sorted : sorted.slice(0, 4);
  return (
    <div className="sources">
      <div className="sources-grid">
        {shown.map((s) => (
          <button key={s.n} className="source-card" onClick={() => open(s)} title={fileSource(s.url)?.path ?? s.url}>
            <span className="num">{s.n}</span>
            <span className="body">
              <span className="title">{s.title || hostOf(s.url)}</span>
              <span className="host">
                {hostOf(s.url)}
                {s.read && <em> · read</em>}
              </span>
            </span>
          </button>
        ))}
      </div>
      {sources.length > 4 && (
        <button className="btn sm ghost" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${sources.length} sources`}
        </button>
      )}
    </div>
  );
}
