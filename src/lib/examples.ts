// Example prompts for the home screen and the help center: only ones that work with the modules that are on.
export interface Example {
  text: string;
  /** A settings flag that must be on (true by default unless noted in `offByDefault`). */
  needs?: string;
  /** Needs the web (Web: Auto or Always). */
  web?: boolean;
  /** Only on a Mac. */
  mac?: boolean;
  group: string;
}

/** Modules that are off unless the user turned them on. */
const OFF_BY_DEFAULT = new Set(["readAloud", "wakeWord", "clipboardHistory"]);

export const EXAMPLES: Example[] = [
  { group: "Research", text: "What are the biggest tech news stories this week?", web: true },
  { group: "Research", text: "What does research say about intermittent fasting?", web: true },
  { group: "Research", text: "Is it true that we only use 10% of our brains?", web: true },
  { group: "Research", text: "MacBook Air vs Dell XPS 13 for a student", web: true },
  { group: "Research", text: "Summarize https://www.youtube.com/watch?v=… with timestamps", web: true },
  { group: "Research", text: "Is the Sony WH-1000XM5 worth it? What do reviews say?", web: true, needs: "reviewsEnabled" },
  { group: "Research", text: "Plan 3 days in Lisbon in May for two, around $1,500", web: true },
  { group: "Research", text: "Coffee shops near me that are open now", web: true },
  { group: "Thinking", text: "Help me decide between renting and buying a home. What should I consider?" },
  { group: "Thinking", text: "Explain how a mortgage works like I'm 15" },
  { group: "Thinking", text: "A bat and a ball cost $1.10 in total. The bat costs $1 more than the ball. How much is the ball?" },
  { group: "Writing", text: "Write a friendly email asking my landlord to fix a leaking sink." },
  { group: "Writing", text: "Write a two-minute toast for my sister's wedding", needs: "writingEnabled" },
  { group: "Writing", text: "Rewrite this to sound more confident: …", needs: "writingEnabled" },
  { group: "Writing", text: "Translate this into Spanish: Where is the nearest train station?", needs: "translateEnabled" },
  { group: "Learning", text: "Make 10 flashcards about the French Revolution", needs: "studyEnabled" },
  { group: "Learning", text: "Quiz me on the solar system, 5 questions", needs: "studyEnabled" },
  { group: "Learning", text: "Help me solve 3x + 7 = 22 step by step, don't just give the answer", needs: "studyEnabled" },
  { group: "Kitchen", text: "What can I make with eggs, spinach and feta?", needs: "kitchenEnabled" },
  { group: "Kitchen", text: "Plan this week's dinners with chicken, rice and broccoli", needs: "kitchenEnabled" },
  { group: "Kitchen", text: "How do I make a proper flat white at home?", needs: "kitchenEnabled" },
  { group: "Plans", text: "Make a 4-week plan to start running 5 km, three days a week." },
  { group: "Plans", text: "Remind me to call Mom tomorrow at 3pm", needs: "macControl", mac: true },
  { group: "Plans", text: "What's on my calendar this week?", needs: "macControl", mac: true },
  { group: "Plans", text: "Every weekday at 8am give me my daily briefing", needs: "tasksEnabled" },
  { group: "Plans", text: "Add \"renew passport\" to my to-do list", needs: "tasksEnabled" },
  { group: "Plans", text: "Track my package 1Z999AA10123456784", needs: "trackersEnabled" },
  { group: "Your Mac", text: "What's taking up space on my Mac?", needs: "macUpkeep", mac: true },
  { group: "Your Mac", text: "Why is my Mac slow right now?", needs: "macUpkeep", mac: true },
  { group: "Your Mac", text: "Turn on dark mode", needs: "macControl", mac: true },
  { group: "Your Mac", text: "Organize my Downloads folder", needs: "macControl", mac: true },
  { group: "Your Mac", text: "Reply to Sam's latest email saying I can make it", needs: "macControl", mac: true },
  { group: "Documents", text: "Make a 10-slide deck about saving for retirement" },
  { group: "Documents", text: "Write a 3-page PDF report on electric cars, with sources", web: true },
  { group: "Your files", text: "What does my lease say about pets?", needs: "kbEnabled" },
  { group: "Ideas", text: "Brainstorm names for a coffee truck" },
  { group: "Ideas", text: "Give me 10 weekend project ideas for a rainy day" },
];

/** Examples that work here: their module is on, web is on if needed, and Mac-only ones only on a Mac. */
export function availableExamples(settings: Record<string, unknown> | null | undefined, opts: { web: boolean; mac: boolean }, all: Example[] = EXAMPLES): Example[] {
  return all.filter((e) => {
    if (e.web && !opts.web) return false;
    if (e.mac && !opts.mac) return false;
    if (!e.needs) return true;
    const v = settings?.[e.needs];
    return v === undefined ? !OFF_BY_DEFAULT.has(e.needs) : v !== false;
  });
}

/** `n` examples for today, from different groups where possible; the same all day, different tomorrow. */
export function todaysPicks(list: Example[], n: number, day = Math.floor(Date.now() / 86_400_000)): Example[] {
  if (!list.length) return [];
  const start = (day * 7) % list.length;
  const rotated = [...list.slice(start), ...list.slice(0, start)];
  const out: Example[] = [];
  const groups = new Set<string>();
  for (const e of rotated) if (out.length < n && !groups.has(e.group)) (out.push(e), groups.add(e.group));
  for (const e of rotated) if (out.length < n && !out.includes(e)) out.push(e);
  return out;
}
