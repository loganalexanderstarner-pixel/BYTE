import { Lightbulb, Rocket, Sparkles, Telescope } from "lucide-react";

import { Logo } from "../../design/Logo";
import { useStore } from "../../state/store";

const SUGGESTIONS = [
  { icon: Lightbulb, title: "Explain something", prompt: "Explain how compound interest works, with a simple example.", hint: "Clear explanations with examples" },
  { icon: Telescope, title: "Think it through", prompt: "Help me decide between renting and buying a home. What should I consider?", hint: "Step-by-step reasoning" },
  { icon: Sparkles, title: "Write for me", prompt: "Write a friendly email asking my landlord to fix a leaking sink.", hint: "Emails, essays, messages" },
  { icon: Rocket, title: "Plan something", prompt: "Make a 4-week plan to start running 5 km, three days a week.", hint: "Schedules, checklists, goals" },
];

function greeting(): string {
  const h = new Date().getHours();
  if (h < 5) return "Up late?";
  if (h < 12) return "Good morning.";
  if (h < 18) return "Good afternoon.";
  return "Good evening.";
}

export function EmptyState() {
  const send = useStore((s) => s.send);
  const engine = useStore((s) => s.engine);
  const ready = engine.state === "ready";
  return (
    <div className="empty">
      <Logo size={64} />
      <h1>{greeting()}</h1>
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
