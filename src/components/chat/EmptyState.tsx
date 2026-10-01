import { Lightbulb, MessageCircle, Rocket, Sparkles, Telescope } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../../lib/api";
import { availableExamples, todaysPicks } from "../../lib/examples";
import type { Assistant } from "../../lib/types";

import { Logo } from "../../design/Logo";
import { Deck } from "./Deck";
import { canSpeak, spaceOf, useStore, workspaceOf } from "../../state/store";

const ICONS = [Lightbulb, Telescope, Sparkles, Rocket];
const DEFAULT_SUGGESTIONS = [
  { icon: Lightbulb, title: "What's new", prompt: "What are the biggest tech news stories this week?", hint: "Searches the web, with sources" },
  { icon: Telescope, title: "Think it through", prompt: "Help me decide between renting and buying a home. What should I consider?", hint: "Step-by-step reasoning" },
  { icon: Sparkles, title: "Write for me", prompt: "Write a friendly email asking my landlord to fix a leaking sink.", hint: "Emails, essays, messages" },
  { icon: Rocket, title: "Plan something", prompt: "Make a 4-week plan to start running 5 km, three days a week.", hint: "Schedules, checklists, goals" },
];

function greeting(name?: string | null): string {
  const h = new Date().getHours();
  const who = name?.trim() ? `, ${name.trim()}` : "";
  if (h < 5) return `Up late${who}?`;
  if (h < 12) return `Good morning${who}.`;
  if (h < 18) return `Good afternoon${who}.`;
  return `Good evening${who}.`;
}

export function EmptyState() {
  const send = useStore((s) => s.send);
  const engine = useStore((s) => s.engine);
  const ready = engine.state === "ready";
  const userName = useStore((s) => s.settings?.userName);
  const settings = useStore((s) => s.settings);
  // Today's ideas, from what works with the modules that are on (rotates daily).
  const picks = todaysPicks(availableExamples(settings as unknown as Record<string, unknown>, { web: settings?.webSearch !== false, mac: canSpeak() }), 4);
  const SUGGESTIONS = picks.length === 4 ? picks.map((e, i) => ({ icon: ICONS[i], title: e.group, prompt: e.text.replace("…", ""), hint: e.text.length > 60 ? `${e.text.slice(0, 58)}…` : e.text, fill: e.text.includes("…") })) : DEFAULT_SUGGESTIONS.map((x) => ({ ...x, fill: false }));
  const space = useStore((s) => {
    const c = s.conversations.find((x) => x.id === s.currentId);
    return c?.private ? "local" : c ? spaceOf(c.id) : workspaceOf(s.settings);
  });
  const assistantId = useStore((s) => s.conversations.find((x) => x.id === s.currentId)?.assistantId ?? null);
  const [assistant, setAssistant] = useState<Assistant | null>(null);
  useEffect(() => {
    if (!assistantId) return setAssistant(null);
    api.assistantsList().then((l) => setAssistant(l.find((a) => a.id === assistantId) ?? null), () => setAssistant(null));
  }, [assistantId]);
  if (assistant) {
    return (
      <div className="empty">
        <div className="assistant-hero">{assistant.emoji}</div>
        <h1>{assistant.name}</h1>
        <p className="muted">{assistant.instructions.split(/(?<=[.!?])\s/)[0]}</p>
        <div className="suggestions">
          {assistant.starters.map((prompt) => (
            <button key={prompt} className="suggestion" disabled={!ready} onClick={() => void send(prompt)} title={prompt}>
              <b>
                <MessageCircle size={16} style={{ color: "var(--accent)" }} />
                {prompt.length > 48 ? `${prompt.slice(0, 46)}…` : prompt}
              </b>
            </button>
          ))}
        </div>
      </div>
    );
  }
  const where =
    space === "cloud"
      ? "Answers come from your BYTE cloud."
      : space === "both"
        ? "This Mac and your cloud both answer; keep the better one."
        : "Everything stays on this Mac.";
  return (
    <div className="empty">
      <Logo size={64} />
      <h1>{greeting(userName)}</h1>
      <p className="muted">What can I help you with? {where}</p>
      {space !== "cloud" && <Deck />}
      <div className="suggestions">
        {SUGGESTIONS.map(({ icon: Icon, title, prompt, hint, fill }) => (
          <button key={prompt} className="suggestion" disabled={!ready} onClick={() => (fill ? useStore.setState({ prefill: prompt }) : void send(prompt))} title={prompt}>
            <b>
              <Icon size={16} style={{ color: "var(--accent)" }} />
              {title}
            </b>
            <span>{hint}</span>
          </button>
        ))}
      </div>
      <button className="linklike faint small" style={{ marginTop: 10 }} onClick={() => useStore.getState().openHelp("ideas")}>
        More ideas to try
      </button>
    </div>
  );
}
