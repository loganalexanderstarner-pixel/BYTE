import { openUrl } from "@tauri-apps/plugin-opener";
import { Clapperboard, GraduationCap, Layers as LayersIcon, Gamepad2, ShieldCheck, Store, Star, Layers, AppWindow, ArrowLeft, Download, Eye, FileDown, Keyboard, MousePointerClick, ListCollapse, MoveVertical, BookOpen, Calculator, CalendarDays, Check, ChefHat, Lightbulb as IdeaIcon, ChevronRight, Columns3, ListTree, MapPin, Plane, Scale, CircleCheck, CloudSun, Copy, FolderSearch, Globe, ListChecks, ListFilter, LoaderCircle, Quote, Search, SearchCheck, TriangleAlert } from "lucide-react";
import { Fragment, useState } from "react";

import { CITE_STYLES, cite, citeAll, plainCitation, type CiteStyle } from "../../lib/citations";

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
    case "academic_search":
      return { icon: BookOpen, text: `Searched research papers for “${arg("query")}”` };
    case "plan_research":
      return { icon: ListChecks, text: "Planned the research" };
    case "rank_passages":
      return { icon: ListFilter, text: "Picked the most relevant passages" };
    case "find_gaps":
      return { icon: SearchCheck, text: "Checked what's still missing" };
    case "extract_claims":
      return { icon: ListTree, text: "Found the claims to check" };
    case "plan_comparison":
      return { icon: Columns3, text: "Set up the comparison" };
    case "score_options":
      return { icon: Scale, text: "Scored each option" };
    case "find_places":
      return { icon: MapPin, text: `Looked up ${arg("what")} near ${arg("near") || "you"}` };
    case "plan_trip":
      return { icon: Plane, text: "Read the trip details" };
    case "trip_weather":
      return { icon: CloudSun, text: `Checked the weather for ${arg("place")}` };
    case "write_itinerary":
      return { icon: CalendarDays, text: "Planned each day" };
    case "write_recipe":
      return { icon: ChefHat, text: `Wrote the recipe for ${arg("dish")}` };
    case "recipe_ideas":
      return { icon: IdeaIcon, text: "Came up with dishes you can make" };
    case "meal_plan":
      return { icon: CalendarDays, text: "Planned the meals" };
    case "get_transcript":
      return { icon: Clapperboard, text: "Read the video's captions" };
    case "summarize_video":
      return { icon: ListChecks, text: "Summarized the video" };
    // The web agent: once a step finished, its result says exactly what happened.
    case "open_url":
      return { icon: AppWindow, text: s.status === "running" ? `Opening ${hostOf(arg("url"))}` : (s.summary ?? `Opened ${hostOf(arg("url"))}`) };
    case "click":
      return { icon: MousePointerClick, text: s.summary ?? `Clicking element ${arg("n")}` };
    case "type_text":
      return { icon: Keyboard, text: s.summary ?? `Typing “${arg("text")}”` };
    case "choose_option":
      return { icon: ListCollapse, text: s.summary ?? `Choosing “${arg("option")}”` };
    case "look_at_page":
      return { icon: Eye, text: s.summary ?? "Looking at the page" };
    case "scroll_page":
      return { icon: MoveVertical, text: s.summary ?? "Scrolling" };
    case "go_back":
      return { icon: ArrowLeft, text: s.summary ?? "Going back" };
    case "download_file":
      return { icon: Download, text: s.summary ?? "Getting the download ready" };
    case "look_at_photo":
      return { icon: Eye, text: s.summary ?? `Looking at ${arg("name") || "the photo"}` };
    case "make_flashcards":
      return { icon: LayersIcon, text: `Made flashcards on ${arg("topic") || "this"}` };
    case "tutor_step":
      return { icon: GraduationCap, text: "Tutor: one step at a time" };
    case "make_quiz":
      return { icon: GraduationCap, text: `Wrote a quiz on ${arg("topic") || "this"}` };
    case "find_prices":
      return { icon: Store, text: `Read the prices for ${arg("product")}` };
    case "summarize_reviews":
      return { icon: Star, text: `Summarized the reviews of ${arg("product")}` };
    case "write_hints":
      return { icon: Gamepad2, text: "Wrote spoiler-free hints" };
    case "write_drafts":
      return { icon: Layers, text: "Wrote 3 drafts and compared their answers" };
    case "check_answer":
      return { icon: ShieldCheck, text: "Checked the answer against its sources" };
    case "save_page":
      return { icon: FileDown, text: s.summary ?? "Saving the page" };
    default:
      return { icon: CircleCheck, text: s.name };
  }
}

const BROWSER_STEPS = ["open_url", "click", "type_text", "choose_option", "look_at_page", "scroll_page", "go_back", "download_file", "save_page"];

/** Summary line like "Searched the web twice · read 3 pages". */
export function activitySummary(steps: Step[]): string {
  const searches = steps.filter((s) => s.name === "web_search").length;
  const reads = steps.filter((s) => s.name === "read_page" && s.status === "ok").length;
  const calcs = steps.filter((s) => s.name === "calculate").length;
  const weather = steps.some((s) => s.name === "weather" && s.status === "ok");
  const files = steps.some((s) => s.name === "search_my_files");
  const papers = steps.some((s) => s.name === "academic_search" && s.status === "ok");
  const researched = steps.some((s) => s.name === "plan_research");
  const checked = steps.some((s) => s.name === "extract_claims");
  const compared = steps.some((s) => s.name === "plan_comparison");
  const tripped = steps.some((s) => s.name === "plan_trip");
  const mapped = steps.some((s) => s.name === "find_places" && s.status === "ok");
  const parts: string[] = [];
  const cooked = steps.some((s) => ["write_recipe", "recipe_ideas", "meal_plan"].includes(s.name));
  const browsed = steps.filter((s) => BROWSER_STEPS.includes(s.name)).length;
  if (browsed) parts.push(`Used the browser (${browsed} step${browsed === 1 ? "" : "s"})`);
  if (steps.some((s) => s.name === "look_at_photo" && s.status === "ok")) parts.push("Looked at the photo");
  if (steps.some((s) => s.name === "make_flashcards")) parts.push("Made flashcards");
  if (steps.some((s) => s.name === "make_quiz")) parts.push("Wrote a quiz");
  if (steps.some((s) => s.name === "summarize_reviews")) parts.push("Read the reviews");
  if (steps.some((s) => s.name === "find_prices")) parts.push("Compared prices");
  if (steps.some((s) => s.name === "write_hints")) parts.push("Found hints");
  if (steps.some((s) => s.name === "write_drafts")) parts.push("Wrote 3 drafts");
  if (steps.some((s) => s.name === "check_answer")) parts.push("checked against sources");
  if (cooked) parts.push("In the kitchen");
  if (steps.some((s) => s.name === "get_transcript" && s.status === "ok")) parts.push("Watched the video");
  if (tripped) parts.push("Planned the trip");
  else if (mapped) parts.push("Checked the map");
  if (checked) parts.push("Fact-checked");
  else if (compared) parts.push("Compared");
  else if (researched) parts.push("Researched");
  if (files) parts.push("Searched your files");
  if (weather) parts.push("Checked the forecast");
  if (searches) parts.push(searches === 1 ? "Searched the web" : `Searched the web ${searches} times`);
  if (papers) parts.push("searched papers");
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
                {s.summary && !BROWSER_STEPS.includes(s.name) && <span className="result">{s.summary}</span>}
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
  const [citing, setCiting] = useState(false);
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
                {s.meta ? paperLine(s) : hostOf(s.url)}
                {s.read && !s.meta && <em> · read</em>}
              </span>
            </span>
          </button>
        ))}
      </div>
      <div className="sources-actions">
        {sources.length > 4 && (
          <button className="btn sm ghost" onClick={() => setAll(!all)}>
            {all ? "Show fewer" : `Show all ${sources.length} sources`}
          </button>
        )}
        <button className="btn sm ghost" onClick={() => setCiting(!citing)} aria-expanded={citing}>
          <Quote size={13} /> Cite
        </button>
      </div>
      {citing && <CiteMenu sources={sorted.filter((s) => !fileSource(s.url))} />}
    </div>
  );
}

/** "Paper · 2023 · Nature" for a research paper's card. */
function paperLine(s: Source): string {
  return ["Paper", s.meta?.year, s.meta?.venue].filter(Boolean).join(" · ");
}

/** Shows `*italic*` markers from the citation formatter as italics. */
function Styled({ text }: { text: string }) {
  return (
    <>
      {text.split(/\*([^*]+)\*/).map((part, i) => (i % 2 ? <em key={i}>{part}</em> : <Fragment key={i}>{part}</Fragment>))}
    </>
  );
}

/** Citations for an answer's sources in a chosen style, copyable one by one or all at once. */
function CiteMenu({ sources }: { sources: Source[] }) {
  const [style, setStyle] = useState<CiteStyle>(() => {
    try {
      return (localStorage.getItem("byte.citeStyle") as CiteStyle) || "apa";
    } catch {
      return "apa";
    }
  });
  const [copied, setCopied] = useState<number | "all" | null>(null);
  const pick = (s: CiteStyle) => {
    setStyle(s);
    try {
      localStorage.setItem("byte.citeStyle", s);
    } catch {
      /* per-viewer convenience only */
    }
  };
  const copy = (text: string, which: number | "all") => {
    void navigator.clipboard?.writeText(plainCitation(text)).then(() => {
      setCopied(which);
      setTimeout(() => setCopied(null), 1500);
    });
  };
  if (sources.length === 0) return null;
  const ordered = [...sources].sort((a, b) => a.n - b.n);
  return (
    <div className="cite-menu" role="region" aria-label="Citations">
      <div className="cite-head">
        <div className="segmented" aria-label="Citation style">
          {CITE_STYLES.map((c) => (
            <button key={c.id} aria-pressed={style === c.id} onClick={() => pick(c.id)}>
              {c.label}
            </button>
          ))}
        </div>
        <button className="btn sm" onClick={() => copy(citeAll(ordered, style), "all")}>
          {copied === "all" ? <Check size={13} /> : <Copy size={13} />} Copy all
        </button>
      </div>
      <ol className={`cite-list ${style === "bibtex" ? "mono" : ""}`}>
        {ordered.map((s) => {
          const text = cite(s, style);
          return (
            <li key={s.n}>
              <span className="num">{s.n}</span>
              <span className="text">{style === "bibtex" ? <pre>{text}</pre> : <Styled text={text} />}</span>
              <button className="icon-btn" title="Copy" aria-label={`Copy citation ${s.n}`} onClick={() => copy(text, s.n)}>
                {copied === s.n ? <Check size={13} /> : <Copy size={13} />}
              </button>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
