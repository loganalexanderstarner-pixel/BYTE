import { Lightbulb, Rocket, Sparkles, Telescope } from "lucide-react";

import { Logo } from "../../design/Logo";
import { useStore } from "../../state/store";

const SUGGESTIONS = [
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
  return (
    <div className="empty">
      <Logo size={64} />
      <h1>{greeting(userName)}</h1>
      <p className="muted">What can I help you with? Everything stays on this Mac.</p>
      <div className="suggestions">
        {SUGGESTIONS.map(({ icon: Icon, title, prompt, hint }) => (
          <button key={title} className="suggestion" disabled={!ready} onClick={() => void send(prompt)} title={prompt}>
            <b>
              <Icon size={16} style={{ color: "var(--accent)" }} />
              {title}
            </b>
            <span>{hint}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
