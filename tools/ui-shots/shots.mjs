// Renders the built UI with a mocked Tauri backend and saves screenshots.
// See README.md in this folder. Every Tauri command a screen uses must be
// mocked in `initScript` below (unknown commands return null).
import { chromium } from "playwright";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync } from "node:zlib";

const here = dirname(fileURLToPath(import.meta.url));
// A real `models_list` reply for a 16 GB M4 (regenerate if the ModelStatus shape changes).
const REAL = JSON.parse(gunzipSync(readFileSync(process.env.MODELS ?? join(here, "models16.json.gz"))).toString("utf8"));
// Model details (the dropdown) come from the current catalog, so the snapshot above needn't be rebuilt.
const CATALOG = JSON.parse(readFileSync(join(here, "../../src-tauri/catalog/models.json"), "utf8"));
const DETAILS = new Map(CATALOG.models.map((m) => [m.id, { details: m.details ?? null, ...(m.tags ? { tags: m.tags } : {}) }]));
const VISION = new Map(CATALOG.models.filter((m) => m.vision).map((m) => [m.id, m.vision.file.size]));

const URL = process.env.URL ?? "http://localhost:4173/";
const OUT = process.env.OUT ?? join(here, "out");
mkdirSync(OUT, { recursive: true });

function mock(onboarded, theme) {
  const GB = 1e9;
  const fit = (f, ctx, note) => ({ fit: f, context: ctx, neededBytes: 11.1 * GB, gpuBudgetBytes: 11.45 * GB, totalRamBytes: 17.18 * GB, note });
  const models = JSON.parse(JSON.stringify(REAL.models));
  for (const m of models) if (DETAILS.has(m.id)) Object.assign(m, DETAILS.get(m.id));
  for (const m of models)
    if (VISION.has(m.id)) m.vision = { key: `${m.id}:vision`, sizeBytes: VISION.get(m.id), installed: onboarded && m.id === "qwen3.5-9b", downloading: false };
  for (const m of models) for (const v of m.variants) if (v.key === "qwen3.5-9b:Q6_K" && onboarded) v.measuredTps = 21.4;
  if (onboarded) {
    const m = models.find((x) => x.id === "qwen3.5-9b");
    m.variants.find((v) => v.quant === "Q6_K").installed = true;
    const b = models.find((x) => x.id === "qwen3.8-27b");
    b.variants.find((v) => v.quant === "UD-IQ2_S").partialBytes = 3.1e9;
    models.find((x) => x.id === "qwen3.5-4b").variants.find((v) => v.quant === "Q6_K").installed = true;
    // With the 9B loaded, small models fit alongside.
    for (const x of models) for (const v of x.variants) v.fitsAlongside = v.sizeBytes < 4.5e9;
  }
  const day = 86400000;
  const now = Date.now();
  const mk = (id, title, ago, extra = {}) => ({ id, title, createdAt: now - ago, updatedAt: now - ago, pinned: false, folder: null, messageCount: 2, ...extra });
  const chats = onboarded
    ? [
        mk("c1", "M5 MacBook Air: worth upgrading?", 0.1 * day),
        mk("c2", "Weekly meal prep ideas", 0.5 * day, { pinned: true }),
        mk("c3", "Quarterly tax estimate", 1.2 * day, { folder: "Work" }),
        mk("c4", "Client email follow-up", 3 * day, { folder: "Work" }),
        mk("c5", "Lisbon trip plan", 4 * day, { folder: "Travel" }),
        mk("c6", "Rust vs Go for a CLI", 9 * day),
        mk("c7", "Cabinet options: oak vs maple", 0.3 * day, { projectId: "p1", summary: "Comparing oak and maple cabinets for a $20k kitchen remodel.", tags: ["kitchen", "remodel"] }),
        mk("c8", "Countertop quotes", 2 * day, { projectId: "p1" }),
      ]
    : [];
  const memories = onboarded
    ? [
        { id: "m1", text: "Works as a nurse in Denver", source: "chat", createdAt: now - 5 * day },
        { id: "m2", text: "Prefers short answers with bullet points", source: "user", createdAt: now - 4 * day },
        { id: "m3", text: "Uses a MacBook Air M4 with 16 GB", source: "chat", createdAt: now - 1 * day },
      ]
    : [];
  const settings = { autoTune: true, tuning: onboarded ? { "qwen3.5-9b:Q6_K": { boost: true, kvF16: false, ubatch: 1024, tokensPerSec: 21.4, promptPerSec: 412, chip: "Apple M4 10-core GPU", testedAt: Date.now(), flashAttn: true, draftNMax: 16, draftPMin: 0.75, thorough: true, helperKind: "draft", ngram: true } } : {}, speedBoost: true, speedPref: "balanced", memoryEnabled: true, kbEnabled: true, answerCache: true, aboutMe: "I'm Logan. I like clear, practical answers.", loadedAlongside: [], webSearch: true, userName: "Logan", onboardingComplete: onboarded, activeModel: onboarded ? "qwen3.5-9b:Q6_K" : null, contextSize: null, defaultMode: "auto", thinking: "auto", theme, accent: null, fontScale: 1, density: "comfortable", showStats: true, batterySaver: true, modelOverrides: onboarded ? { "qwen3.5-9b:Q6_K": { temperature: 0.6, thinkingBudget: 1024, systemExtra: "Use metric units." } } : {} };
  const system = REAL.system;
  const loaded = onboarded
    ? [
        { key: "qwen3.5-9b:Q6_K", primary: true, status: { state: "ready", model: "qwen3.5-9b:Q6_K", context: 16384 }, context: 16384, neededBytes: 8.6e9 },
        { key: "qwen3.5-4b:Q6_K", primary: false, status: { state: "ready", model: "qwen3.5-4b:Q6_K", context: 8192 }, context: 8192, neededBytes: 4.1e9 },
      ]
    : [];
  const projects = onboarded ? [
    { id: "p1", name: "Kitchen remodel", instructions: "Budget is $20,000. Kitchen is 12 x 14 ft. We like light wood.", createdAt: now - 7 * day },
    { id: "p2", name: "Spanish class", instructions: "I'm a beginner (A2). Explain grammar simply.", createdAt: now - 20 * day },
  ] : [];
  const profiles = { active: "default", profiles: [
    { id: "default", name: "Logan", createdAt: 0 },
    { id: "work-1a2b3c", name: "Work", createdAt: now - 3 * day },
  ] };
  const cloud = { connected: false, baseUrl: "https://byteai.bytebylogan.xyz", account: null };
  Object.assign(settings, { cloudConnected: false, cloudBaseUrl: null, cloudAccount: null, useCloud: false, cloudMode: null, workspace: "local" });
  return { models, settings, system, recommend: REAL.recommend, loaded, chats, memories, projects, profiles, cloud };
}

function initScript({ data }) {
  const callbacks = new Map();
  const listeners = new Map();
  let next = 1;
  window.__emit = (event, payload) => {
    for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
  };
  const answer = [
    "Here's a quick comparison:\n\n",
    "| Option | Upfront cost | Flexibility |\n|---|---|---|\n| **Renting** | Low | High |\n| **Buying** | High | Low |\n\n",
    "## What to weigh\n\n",
    "- **Time horizon.** Buying usually wins only if you stay 5+ years.\n",
    "- **Total cost.** Include taxes, insurance and maintenance (~1–2% of the price per year).\n",
    "- **Opportunity cost.** A down payment invested elsewhere could grow.\n\n",
    "A simple rule of thumb:\n\n```python\nprice_to_rent = home_price / (monthly_rent * 12)\n# under 15 → buying is attractive; over 20 → renting usually wins\n```\n",
  ];
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    plugins: {},
    transformCallback(cb) {
      const id = next++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    convertFileSrc: (p) => p,
    async invoke(cmd, args) {
      switch (cmd) {
        case "plugin:event|listen": {
          const l = listeners.get(args.event) ?? [];
          l.push(args.handler);
          listeners.set(args.event, l);
          return args.handler;
        }
        case "plugin:event|unlisten":
          return null;
        case "settings_get":
          return { ...data.settings };
        case "settings_update":
          data.settings = { ...data.settings, ...args.patch };
          return { ...data.settings };
        case "system_info":
          return data.system;
        case "models_list":
          return data.models;
        case "memory_report":
          return { totalBytes: 17179869184, availableBytes: 1900000000, apps: [
            { name: "Google Chrome", bytes: 2430000000, processes: 23 },
            { name: "Slack", bytes: 910000000, processes: 5 },
            { name: "Spotify", bytes: 420000000, processes: 4 },
            { name: "Visual Studio Code", bytes: 380000000, processes: 9 },
          ] };
        case "app_quit":
          return null;
        case "engine_status":
          if (data.engineError) return { state: "error", message: data.engineError };
          return data.settings.onboardingComplete ? { state: "ready", model: "qwen3.5-9b:Q6_K", context: 16384, boosted: true, vision: !!data.vision } : { state: "noModel" };
        case "speed_boost_info":
          return { enabled: true, available: true, helperKey: "qwen3.5-0.8b:Q8_0", helperName: "Qwen3.5 0.8B", helperBytes: 812000000, installed: true, kind: "draft" };
        case "gpu_share_info":
          return { supported: true, currentBytes: 11453246122, defaultBytes: 11453246122, raisedBytes: 12884901888, raised: false };
        case "engine_tune":
          return data.settings.tuning["qwen3.5-9b:Q6_K"];
        case "chats_list":
          return data.chats;
        case "chat_load": {
          const c = data.chats.find((x) => x.id === args.id);
          if (!c) return null;
          if (args.id === "c7") {
            const t = c.createdAt;
            const old = [
              { id: "u0", role: "user", content: "Oak or maple cabinets?", status: "done", createdAt: t },
              { id: "a0", role: "assistant", content: "Both are solid choices…", status: "done", createdAt: t },
            ];
            return { ...c, messages: [
              { id: "u1", role: "user", content: "Oak or maple cabinets for a light, modern kitchen on a $20k budget?", status: "done", createdAt: t, alts: [old], version: 1 },
              { id: "a1", role: "assistant", content: "**Maple** fits better: its smooth, pale grain looks modern and takes light finishes evenly. Oak's strong grain reads more traditional.\n\n- Maple: about $150–250 per linear foot\n- Oak: about $120–200 per linear foot", status: "done", createdAt: t },
              { id: "u2", role: "user", content: "What about the countertops?", status: "done", createdAt: t },
              { id: "a2", role: "assistant", content: "Quartz in a warm white would pair well with maple and stays within", status: "cancelled", interrupted: true, createdAt: t },
            ] };
          }
          return { ...c, messages: [
            { id: args.id + "u", role: "user", content: "Is the new MacBook Air worth it if I have an M4?", status: "done", createdAt: c.createdAt },
            { id: args.id + "a", role: "assistant", content: "Probably not: the M5 is about 20% faster, but the M4 Air is still excellent for everyday work.", status: "done", createdAt: c.createdAt },
          ] };
        }
        case "chats_search":
          return [
            { conversationId: "c5", messageId: "x", title: "Lisbon trip plan", snippet: "Day 1: explore Alfama and ride tram 28 to «Lisbon»'s old town…", updatedAt: Date.now() },
            { conversationId: "c2", messageId: "y", title: "Weekly meal prep ideas", snippet: "…a «Lisbon»-style bacalhau bake works well for batch cooking", updatedAt: Date.now() },
          ];
        case "projects_list":
          return data.projects;
        case "profiles_list":
          return data.profiles;
        case "chat_autotitle":
          return null;
        case "memories_list":
          return data.memories;
        case "chat_save":
        case "chat_update":
        case "chats_import":
          return null;
        case "models_loaded":
          return data.loaded;
        case "model_recommend":
          return data.recommend;
        case "engine_log":
          return ["main: server is listening on http://127.0.0.1:52811", "srv  update_slots: all slots are idle"];
        case "model_download":
          setTimeout(() => window.__emit("models://download", { kind: "progress", id: args.key ?? args.id, bytes: 3.87e9, total: 9.0e9, bytesPerSec: 48.2e6 }), 50);
          return null;
        case "cloud_status":
          return data.cloud;
        case "cloud_connect":
          data.cloud = { connected: true, baseUrl: "https://byteai.bytebylogan.xyz", account: { name: "Logan", email: "logan@example.com", tier: "Pro", modes: [{ id: "fast", label: "Fast" }, { id: "auto", label: "Auto" }, { id: "extended", label: "Extended" }, { id: "extended_plus", label: "Extended+" }], budgets: { daily_messages: { used: 12, limit: 500 }, documents_left: 40 } } };
          data.settings = { ...data.settings, cloudConnected: true, cloudMode: "auto" };
          return data.cloud;
        case "cloud_action":
          return null;
        case "cloud_conversations":
          return { conversations: [
            { id: 42, title: "Why do spring tides happen?", updated_at: new Date().toISOString() },
            { id: 41, title: "Kubernetes ingress for a home lab", updated_at: new Date(Date.now() - 86400000).toISOString() },
            { id: 40, title: "Birthday party ideas for a 7-year-old", updated_at: new Date(Date.now() - 3 * 86400000).toISOString() },
          ] };
        case "cloud_import": {
          const id = "cloud-" + args.conversationId;
          if (!data.chats.some((c) => c.id === id)) data.chats.unshift({ id, cloudId: String(args.conversationId), title: "Kubernetes ingress for a home lab", createdAt: Date.now(), updatedAt: Date.now(), messageCount: 2 });
          return id;
        }
        case "cloud_delete":
          return null;
        case "cloud_image": {
          const hue = [...args.path].reduce((a, c) => a + c.charCodeAt(0), 0) % 360;
          const page = /preview\/\d/.test(args.path);
          const svg = page
            ? `<svg xmlns='http://www.w3.org/2000/svg' width='300' height='400'><rect width='300' height='400' fill='white'/><rect x='24' y='30' width='200' height='18' fill='hsl(${hue},60%,45%)'/><rect x='24' y='70' width='252' height='8' fill='#ccc'/><rect x='24' y='86' width='230' height='8' fill='#ccc'/><rect x='24' y='102' width='240' height='8' fill='#ccc'/><rect x='24' y='130' width='252' height='120' fill='hsl(${hue},50%,85%)'/></svg>`
            : `<svg xmlns='http://www.w3.org/2000/svg' width='160' height='100'><defs><linearGradient id='g'><stop offset='0' stop-color='hsl(${hue},70%,45%)'/><stop offset='1' stop-color='hsl(${(hue + 60) % 360},70%,30%)'/></linearGradient></defs><rect width='160' height='100' fill='url(#g)'/><rect x='14' y='18' width='90' height='10' fill='white' opacity='.85'/><rect x='14' y='36' width='60' height='6' fill='white' opacity='.6'/></svg>`;
          return "data:image/svg+xml;utf8," + encodeURIComponent(svg);
        }
        case "cloud_attach":
          return { conversationId: "42", attachment: { id: String(100 + Math.floor(Math.random() * 900)), filename: args.file.split("/").pop(), content_type: "image/jpeg" } };
        case "plugin:dialog|open":
          return data.dialogPaths ?? ["/Users/logan/Pictures/tide-pool.jpg"];
        case "doc_outline":
          return { title: "Saving for Your First Home", subtitle: "A practical plan for the next three years", sections: [
            { title: "How much you need", notes: "Down payment, closing costs, emergency fund" },
            { title: "Where to keep the money", notes: "High-yield savings, CDs, what to avoid" },
            { title: "A monthly plan", notes: "Budget, automatic transfers, milestones" },
            { title: "Help you might qualify for", notes: "First-time buyer programs" },
          ] };
        case "doc_write": {
          const send = (e) => args.onEvent.onmessage(e);
          const o = args.request.outline;
          for (let i = 0; i < o.sections.length; i++) { send({ kind: "section", index: i, total: o.sections.length, title: o.sections[i].title }); await new Promise((r) => setTimeout(r, 40)); }
          return { kind: args.request.kind, title: o.title, subtitle: o.subtitle, sources: [{ n: 1, title: "Buying your first home", url: "https://www.consumerfinance.gov/owning-a-home/" }], sections: [
            { title: o.sections[0].title, blocks: [
              { type: "paragraph", text: "Most lenders want 3–20% of the price as a down payment, plus 2–5% for closing costs [1]. Keep three months of expenses aside as well, so a surprise bill doesn't touch the house fund." },
              { type: "table", columns: ["Home price", "5% down", "Closing (3%)", "Total"], rows: [["$250,000", "$12,500", "$7,500", "$20,000"], ["$350,000", "$17,500", "$10,500", "$28,000"]] },
              { type: "callout", text: "Aim for $20,000–$28,000 for a $250k–$350k home." },
            ] },
            { title: o.sections[1].title, blocks: [
              { type: "bullets", items: ["High-yield savings: easy access, about 4% a year", "CDs: a bit more interest, money locked for months", "Avoid stocks for money you need within 3 years"] },
              { type: "chart", chart: "bar", title: "Savings after 3 years at $600/month", labels: ["Checking", "High-yield savings", "CD ladder"], values: [21600, 23900, 24200] },
            ] },
            { title: o.sections[2].title, blocks: [ { type: "numbered", items: ["Set up an automatic transfer on payday", "Cut two subscriptions", "Check progress every 3 months"] } ] },
          ] };
        }
        case "recipes_list":
          return (data.savedRecipes ?? []).slice();
        case "recipe_save": {
          data.savedRecipes = data.savedRecipes ?? [];
          const id = data.savedRecipes.length + 1;
          data.savedRecipes.unshift({ id, title: args.recipe.title, category: args.recipe.category, image: args.recipe.image, savedAt: Date.now(), recipe: args.recipe });
          data.savedRecipes.push({ id: 90, title: "Iced Oat Milk Latte", category: "coffee", image: "", savedAt: Date.now() - 86400000, recipe: { ...args.recipe, title: "Iced Oat Milk Latte", category: "coffee", emoji: "🧋" } });
          data.savedRecipes.push({ id: 91, title: "Brown Butter Chocolate Chip Cookies", category: "baking", image: "", savedAt: Date.now() - 2 * 86400000, recipe: { ...args.recipe, title: "Brown Butter Chocolate Chip Cookies", category: "baking", emoji: "🍪" } });
          return id;
        }
        case "recipe_delete":
          return null;
        case "decks_list":
          return [{ id: 1, name: "The French Revolution", cards: 6, due: 2, new: 4, created: Date.now() }, { id: 2, name: "Spanish verbs", cards: 40, due: 0, new: 12, created: Date.now() - 86400000 }];
        case "deck_save":
          return 1;
        case "study_queue":
          return [{ id: 11, deckId: 1, front: "What was the Reign of Terror?", back: "1793–94: mass executions of suspected enemies of the Revolution, led by Robespierre's Committee of Public Safety.", ease: 2.5, interval: 6, reps: 2, lapses: 0, due: 0 }];
        case "card_review":
          return {};
        case "mac_undo":
          return true;
        case "agent_approve":
          setTimeout(() => window.__agentContinue?.(), 50);
          return true;
        case "agent_show":
          return true;
        case "doc_save":
          return null;
        case "writing_outline":
          return {
            title: "Why Sleep Is Your Secret Study Tool",
            sections: [
              { heading: "The all-nighter myth", points: ["why cramming feels productive", "what the research says"] },
              { heading: "What your brain does at night", points: ["memory consolidation", "clearing waste"] },
              { heading: "Small changes that work", points: ["a fixed wake time", "screens off 30 minutes before bed"] },
            ],
          };
        case "writing_section": {
          const send = (e) => args.onEvent.onmessage(e);
          const part = [
            "# Why Sleep Is Your Secret Study Tool\n\n## The all-nighter myth\n\nIt's 2 a.m., the coffee's gone cold, and you're rereading the same page for the third time. Pulling an all-nighter feels like the responsible choice, but the research says the opposite: a tired brain holds on to far less of what it reads.",
            "## What your brain does at night\n\nWhile you sleep, your brain replays what you learned during the day and files it away for later. Skip that, and much of the day's studying never sticks.",
            "## Small changes that work\n\nPick a wake time and keep it, even on weekends. Put your phone away half an hour before bed. Your grades will thank you.",
          ][args.index] ?? "";
          send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q4_K_M" });
          send({ kind: "content", delta: part });
          send({ kind: "done", finishReason: "stop" });
          return null;
        }
        case "style_learn":
          return "- Voice: warm, direct, a little funny\n- Sentences: short, often one line\n- Habits: opens with \"Hey\"; dashes over commas";
        case "writing_run": {
          const send = (e) => args.onEvent.onmessage(e);
          send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q4_K_M" });
          send({ kind: "content", delta: "Hi team, Tuesday afternoon's meeting has moved to Thursday at 10 a.m., same room, because several of you couldn't make Tuesday. Let me know if Thursday doesn't work for you." });
          send({ kind: "done", finishReason: "stop" });
          return null;
        }
        case "assistants_list":
          return [
            { id: "a1", name: "Email helper", emoji: "✉️", instructions: "Help me write and reply to emails. Short paragraphs, a clear ask, friendly but professional.", starters: ["Reply to this email politely saying no:", "Write a follow-up after a job interview", "Ask my landlord to fix the heating"], mode: "fast", created: 1 },
            { id: "a2", name: "Steelers stats nerd", emoji: "🏈", instructions: "Talk football with me like a friend who knows every stat. Use tables for numbers.", starters: ["How did the defense do last season?"], mode: "auto", created: 2 },
          ];
        case "assistant_presets":
          return [
            { id: "email", name: "Email helper", emoji: "✉️", instructions: "…", starters: [], mode: "fast", created: 0 },
            { id: "coach", name: "Study coach", emoji: "🎓", instructions: "…", starters: [], mode: "auto", created: 0 },
            { id: "code", name: "Coding buddy", emoji: "🧑‍💻", instructions: "…", starters: [], mode: "auto", created: 0 },
            { id: "fitness", name: "Fitness planner", emoji: "🏃", instructions: "…", starters: [], mode: "auto", created: 0 },
          ];
        case "jobs_list": {
          const soon = new Date(Date.now() + 3 * 86400000).toISOString().slice(0, 10);
          const job = (id, company, role, status, extra = {}) => ({ id, company, role, location: "", pay: "", url: "https://jobs.example.com/" + id, status, deadline: "", applied: "", summary: "", requirements: [], notes: "", updated: 0, ...extra });
          return [
            job(1, "Brightline Analytics", "Junior Data Analyst", "saved", { location: "Pittsburgh, PA", pay: "$58k–$66k", deadline: soon }),
            job(2, "Northwind", "Marketing Coordinator", "saved", { location: "Remote", deadline: "2026-12-01" }),
            job(3, "Acme Robotics", "Operations Associate", "applied", { location: "Cleveland, OH", pay: "$52k" }),
            job(4, "Keystone Health", "Patient Services Rep", "interview", { location: "Pittsburgh, PA" }),
          ];
        }
        case "looker_status":
          return { model: null, downloads: ["qwen3.5-0.8b:Q4_K_M", "qwen3.5-0.8b:vision"], downloadBytes: 737504352 };
        case "kb_status":
          return data.kb ?? { sources: [], embedKey: "nomic-embed-v1.5:Q8_0", embedBytes: 146146432, embedInstalled: false, embedRunning: false };
        case "lab_inspect":
        case "lab_inspect_url":
          return { id: "hf-mistral-nemo-12b", name: "Mistral Nemo Instruct 12B", source: args.url ? "huggingface" : "file", path: args.path ?? "", repo: "bartowski/Mistral-Nemo-Instruct-2407-GGUF", file: "Mistral-Nemo-Instruct-2407-Q4_K_M.gguf", sizeBytes: 7477208576, architecture: "llama", paramsB: 12.2, quant: "Q4_K_M", layers: 40, contextMax: 131072, thinking: false, fit: "tight", fitNote: "Fits with an 8k context (uses about 9.1 GB). Close other apps for the best speed.", context: 8192, added: false };
        case "lab_add":
          return `${args.model.id}:${args.model.quant}`;
        case "lab_list":
          return [{ id: "local-gemma-3-4b", name: "Gemma 3 4B Instruct", source: "file", path: "/Users/logan/Models/gemma-3-4b-it-Q4_K_M.gguf", repo: "", file: "", sizeBytes: 2489757696, architecture: "gemma3", paramsB: 3.9, quant: "Q4_K_M", layers: 34, contextMax: 131072, thinking: false, fit: "great", fitNote: "Fits comfortably with a 16k context (uses about 3.4 GB)", context: 16384, added: true }];
        case "lab_remove":
          return null;
        case "engine_live":
          return { ramUsedBytes: 11.8e9, ramTotalBytes: 17179869184, engineRssBytes: 6.9e9, gpuBudgetBytes: 11453246122, battery: { percent: 17, charging: false }, batterySaving: true, recommended: { temperature: 0.6, topP: 0.95 } };
        case "kb_add":
          return 3;
        case "kb_remove":
        case "kb_reindex":
          return null;
        case "kb_search":
          return [];
        case "file_ingest": {
          const name = args.path.split("/").pop();
          if (/Lease 2026/.test(args.path))
            return { name, kind: "pdf", pages: 6, truncated: false, text: "[Page 1]\nRESIDENTIAL LEASE AGREEMENT\nThis lease is made between Harbor Street Rentals LLC (Landlord) and the Tenant named below.\n\n[Page 2]\nRent is $1,450 per month, due on the first day of each month. A late fee of $50 applies after the fifth day.\n\n[Page 3]\nPets are allowed with a $300 deposit and written consent of the Landlord.\n\n[Page 4]\nMAINTENANCE AND REPAIRS\nTenant shall keep the unit clean and in good condition.\nTenant shall promptly report any leak or water damage to Landlord in writing within 48 hours. Damage caused or made worse by a late report may be charged to Tenant.\n\n[Page 5]\nLandlord is responsible for repairs to plumbing, roofing and appliances supplied with the unit.\n\n[Page 6]\nThis lease ends on July 31, 2027." };
          if (/\.(png|jpe?g)$/i.test(name))
            return { name, kind: "image", text: "", truncated: false, image: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==" };
          if (/\.xlsx$/i.test(name)) return { name, kind: "sheet", pages: 3, text: "[Sheet 1]\nMonth | Rent", truncated: false };
          return { name, kind: "pdf", pages: 14, text: "[Page 1]\nResidential lease agreement…", truncated: false };
        }
        case "cloud_post": {
          if (/^\/api\/jobs\/[a-z]+$/.test(args.path)) { data.job = { id: 77, kind: "pptx", title: "Solar power for beginners", topic: "Solar power for beginners", status: "awaiting_approval" }; return { id: 77 }; }
          if (args.path.endsWith("/approve")) { data.job = { ...data.job, status: "done", document_id: 5 }; return {}; }
          return {};
        }
        case "cloud_get": {
          const p = args.path;
          if (p === "/api/jobs") return [data.job ?? { id: 76, title: "Quarterly report", status: "running", progress: 45, phase: "Writing section 3" }];
          if (p === "/api/jobs/77") return data.job;
          if (p === "/api/jobs/77/outline") return { outline: [{ title: "What solar panels are" }, { title: "How sunlight becomes electricity" }, { title: "Costs and savings" }, { title: "Installing panels at home" }, { title: "Quiz: check your understanding" }] };
          if (p.startsWith("/api/templates")) return [{ id: "t1", name: "Neon grid" }, { id: "t2", name: "Classroom" }, { id: "t3", name: "Minimal" }];
          if (p === "/api/documents") return [{ id: 5, title: "Solar power for beginners", format: "pptx" }, { id: 4, title: "Kitchen remodel budget", format: "pdf" }];
          if (p === "/api/documents/5/preview") return { pages: 8 };
          if (p === "/api/memories") return [{ id: 1, content: "Logan prefers short answers with a TL;DR." }, { id: 2, content: "Works on a Kubernetes cluster at home." }];
          if (p === "/api/saved-prompts") return [{ id: 1, title: "summarize", prompt: "Summarize this in 5 bullet points with a one-line TL;DR:" }, { id: 2, title: "email-reply", prompt: "Write a friendly, short reply to this email:" }, { id: 3, title: "explain-like-12", prompt: "Explain this like I'm 12:" }];
          if (p === "/api/settings") return { theme: "dark", default_mode: "auto", personal_context: "I'm Logan. I build things on my home cluster." };
          if (p === "/api/attachments") return [{ id: 1, filename: "receipt.jpg", content_type: "image/jpeg" }, { id: 2, filename: "whiteboard.png", content_type: "image/png" }, { id: 3, filename: "floorplan.jpg", content_type: "image/jpeg" }];
          return [];
        }
        case "chat_send": {
          const send = (e) => args.onEvent.onmessage(e);
          const wait = (ms) => new Promise((r) => setTimeout(r, ms));
          if (args.request.cloud) {
            send({ kind: "remote", conversationId: "42", messageId: null, userMessageId: null });
            send({ kind: "remote", conversationId: "42", messageId: "421", userMessageId: "420" });
            send({ kind: "started", thinking: false, model: "BYTE Cloud" });
            send({ kind: "phase", text: "searching: spring tides moon sun alignment" });
            await wait(60);
            send({ kind: "sources", sources: [
              { n: 1, title: "Tides and water levels — NOAA", url: "https://oceanservice.noaa.gov/education/tutorial_tides/", snippet: "", read: true },
              { n: 2, title: "Spring and neap tides", url: "https://www.britannica.com/science/tide", snippet: "", read: true },
            ]});
            for (const c of ["**Spring tides** happen when the Sun, Moon and Earth line up (new and full moon), so their pulls add up [1].\n\n", "- Highest highs and lowest lows\n", "- About twice a month [2]\n"]) { send({ kind: "content", delta: c }); await wait(30); }
            if (args.request.cloud.mode === "keep-streaming") return null;
            send({ kind: "stats", promptTokens: 0, completionTokens: 60, tokensPerSecond: 41.2, promptMs: 0, totalMs: 3100, thinkingMs: 0, draftTokens: 0, draftAccepted: 0 });
            send({ kind: "remote", conversationId: "42", messageId: "421", userMessageId: null });
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.study) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            const q = args.request.messages[args.request.messages.length - 1].content.toLowerCase();
            if (q.includes("quiz")) {
              send({ kind: "toolCall", id: "q1", name: "make_quiz", args: { topic: "the periodic table", count: 4 } }); await wait(5);
              send({ kind: "toolResult", id: "q1", ok: true, summary: "4 questions" });
              send({ kind: "quiz", title: "The Periodic Table", questions: [
                { question: "What is the chemical symbol for gold?", choices: ["Ag", "Au", "Gd", "Go"], answer: 1, explanation: "Au comes from the Latin aurum." },
                { question: "Which group are the noble gases in?", choices: ["Group 1", "Group 2", "Group 17", "Group 18"], answer: 3, explanation: "Group 18 elements have full outer shells." },
                { question: "What does the atomic number count?", choices: ["Neutrons", "Protons", "Electrons + neutrons", "Isotopes"], answer: 1, explanation: "Each element has a unique number of protons." },
                { question: "Which element is a liquid at room temperature?", choices: ["Mercury", "Sodium", "Iron", "Carbon"], answer: 0, explanation: "Mercury (and bromine) are liquid at 20 °C." },
              ] });
              send({ kind: "content", delta: "Good luck! Answer all four, then press **Check my answers**; you can save any you miss as flashcards." });
            } else {
              send({ kind: "toolCall", id: "f1", name: "make_flashcards", args: { topic: "the French Revolution", count: 6 } }); await wait(5);
              send({ kind: "toolResult", id: "f1", ok: true, summary: "6 cards" });
              send({ kind: "flashcards", title: "The French Revolution", cards: [
                { front: "When did the French Revolution begin?", back: "1789, with the Estates-General and the storming of the Bastille on 14 July." },
                { front: "What was the Estates-General?", back: "An assembly of the three estates: clergy, nobility and commoners." },
                { front: "What was the Reign of Terror?", back: "1793–94: mass executions of suspected enemies of the Revolution, led by Robespierre's Committee of Public Safety." },
                { front: "Declaration of the Rights of Man", back: "1789 statement of rights: liberty, property, security and resistance to oppression." },
                { front: "Who seized power in 1799?", back: "Napoleon Bonaparte, in the coup of 18 Brumaire." },
                { front: "What happened to Louis XVI?", back: "He was tried for treason and executed by guillotine in January 1793." },
              ] });
              send({ kind: "content", delta: "Here's a set covering the causes, key events and outcome. Study a few minutes a day and BYTE will bring each card back right before you'd forget it." });
            }
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.shop) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            const q = args.request.messages[args.request.messages.length - 1].content.toLowerCase();
            const src = (n, title, url) => ({ n, title, url, snippet: "", read: true });
            if (q.includes("reviews")) {
              send({ kind: "toolCall", id: "r1", name: "summarize_reviews", args: { product: "Sony WH-1000XM6" } }); await wait(5);
              send({ kind: "toolResult", id: "r1", ok: true, summary: "7 pages, 3 ratings" });
              send({ kind: "sources", sources: [src(1, "Sony WH-1000XM6 review", "https://www.rtings.com/headphones/reviews/sony/wh-1000xm6"), src(2, "Sony WH-1000XM6 review: the new benchmark", "https://www.theverge.com/sony-wh-1000xm6-review"), src(3, "XM6 after 3 months — r/headphones", "https://www.reddit.com/r/headphones/comments/xm6"), src(4, "Sony WH-1000XM6", "https://www.bestbuy.com/site/sony-wh-1000xm6")] });
              send({ kind: "reviews", product: "Sony WH-1000XM6", verdict: "Class-leading noise cancelling and a comfier fit than the XM5, but pricey and the case is bulky.", read: 7,
                ratings: [{ site: "rtings.com", value: 8.4, best: 10, count: null, n: 1 }, { site: "theverge.com", value: 9, best: 10, count: null, n: 2 }, { site: "bestbuy.com", value: 4.7, best: 5, count: 2143, n: 4 }],
                pros: [{ text: "Best-in-class noise cancelling", sources: [1, 2, 3] }, { text: "Comfortable for long flights", sources: [2, 3] }, { text: "30-hour battery", sources: [1, 4] }],
                cons: [{ text: "Expensive at launch", sources: [2, 4] }, { text: "Case is bulkier than the XM5's", sources: [3] }, { text: "Occasional Bluetooth dropouts on Windows", sources: [3] }],
                bestFor: ["Frequent flyers", "Commuters"], skipIf: ["You want small, foldable headphones"] });
              for (const t of ["The **Sony WH-1000XM6** are the headphones to beat for noise cancelling [1][2]. Owners love the comfort on long flights [3]; the main complaints are the price and a bulkier case [2][3]."]) { send({ kind: "content", delta: t }); await wait(10); }
            } else if (q.includes("cheapest")) {
              send({ kind: "toolCall", id: "p1", name: "find_prices", args: { product: "AirPods Pro 3" } }); await wait(5);
              send({ kind: "toolResult", id: "p1", ok: true, summary: "4 prices" });
              send({ kind: "sources", sources: [src(1, "AirPods Pro 3", "https://www.amazon.com/dp/x"), src(2, "AirPods Pro 3", "https://www.bestbuy.com/site/x"), src(3, "AirPods Pro 3", "https://www.apple.com/shop/product/x"), src(4, "AirPods Pro 3 (Refurbished)", "https://www.backmarket.com/x")] });
              send({ kind: "prices", product: "AirPods Pro 3", checkedAt: new Date().toISOString(), offers: [
                { store: "backmarket.com", title: "AirPods Pro 3", price: 189, currency: "USD", inStock: true, condition: "refurbished", url: "https://www.backmarket.com/x", n: 4 },
                { store: "amazon.com", title: "AirPods Pro 3", price: 219.99, currency: "USD", inStock: true, condition: "new", url: "https://www.amazon.com/dp/x", n: 1 },
                { store: "bestbuy.com", title: "AirPods Pro 3", price: 229.99, currency: "USD", inStock: false, condition: "new", url: "https://www.bestbuy.com/site/x", n: 2 },
                { store: "apple.com", title: "AirPods Pro 3", price: 249, currency: "USD", inStock: true, condition: "new", url: "https://www.apple.com/shop/product/x", n: 3 } ] });
              send({ kind: "content", delta: "Cheapest **new** right now is **Amazon at $219.99** [1]. Back Market has one for $189, but it's refurbished [4]. Prices change often; these were checked just now." });
            } else {
              send({ kind: "toolCall", id: "g1", name: "write_hints", args: {} }); await wait(5);
              send({ kind: "toolResult", id: "g1", ok: true, summary: "3 hints" });
              send({ kind: "sources", sources: [src(1, "Water Temple walkthrough", "https://zelda.fandom.com/wiki/Water_Temple"), src(2, "Water Temple guide", "https://www.ign.com/wikis/ocarina/Water_Temple")] });
              send({ kind: "hints", game: "Ocarina of Time", spot: "Water Temple, stuck after the first water level change", sources: [1, 2],
                hints: ["You've changed the water level once. Think about where else in the temple you've seen a Triforce symbol you could play at.", "The middle tower has a room you can only reach when the water is at its lowest.", "Lower the water fully, then go through the middle tower's bottom door to find the key you need."],
                solution: "1. Go to the Triforce symbol on the middle floor of the central tower.\n2. Play Zelda's Lullaby to raise the water to middle level.\n3. …" });
              send({ kind: "content", delta: "Here's a gentle nudge: you've changed the water level once, so look for other places where you could do that again [1]. Stronger hints and the full solution are in the card above; tap them only if you want them." });
            }
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.mac) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            send({ kind: "toolCall", id: "m0", name: "mac_reminder_add", args: { app: "Reminders", what: "Add the reminder \"Call Mom\"" } });
            send({ kind: "approval", id: "mac1", action: "mac", title: "Add the reminder \"Call Mom\"", site: "Reminders", url: "", target: "Reminders",
              fields: [{ label: "Reminder", value: "Call Mom" }, { label: "When", value: "Wed, Sep 30 at 3 PM" }] });
            let finish;
            const ended = new Promise((r) => (finish = r));
            window.__agentContinue = async () => {
              send({ kind: "approvalDone", id: "mac1", ok: true });
              send({ kind: "toolResult", id: "m0", ok: true, summary: "Call Mom, due Wed, Sep 30 at 3 PM" });
              send({ kind: "macDone", app: "Reminders", title: "Add the reminder \"Call Mom\"", detail: "Call Mom, due Wed, Sep 30 at 3 PM", ok: true, undo: "tok1" });
              for (const t of ["Done! I added **Call Mom** to Reminders for tomorrow (Wednesday) at 3 PM."]) { send({ kind: "content", delta: t }); await wait(10); }
              send({ kind: "done", finishReason: "stop" });
              finish(null);
            };
            return ended;
          }
          if (data.agent) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            send({ kind: "browsing", active: true });
            const steps = [
              ["open_url", { url: "https://www.carnegielibrary.org/" }, true, "Opened carnegielibrary.org — Carnegie Library of Pittsburgh"],
              ["click", { n: 14 }, true, "Clicked “Get a Library Card” → Library Cards"],
              ["click", { n: 9 }, true, "Clicked “Apply online” → Apply for a Library Card"],
              ["type_text", { n: 3, text: "Ada Lovelace" }, true, "Typed “Ada Lovelace” into Full name"],
              ["type_text", { n: 5, text: "ada@example.com" }, true, "Typed “ada@example.com” into Email"],
              ["choose_option", { n: 7, option: "Squirrel Hill" }, true, "Chose “Squirrel Hill” in Home branch"],
              ["click", { n: 12 }, null, ""],
            ];
            for (const [i, [name, a, ok, summary]] of steps.entries()) {
              const id = "s" + i;
              send({ kind: "toolCall", id, name, args: a }); await wait(5);
              if (ok !== null) send({ kind: "toolResult", id, ok, summary });
            }
            send({ kind: "sources", sources: [
              { n: 1, title: "Carnegie Library of Pittsburgh", url: "https://www.carnegielibrary.org/", snippet: "", read: true },
              { n: 2, title: "Apply for a Library Card", url: "https://www.carnegielibrary.org/apply/", snippet: "", read: true },
            ]});
            send({ kind: "approval", id: "ap1", action: "submit", title: "Submit the form on carnegielibrary.org?", site: "carnegielibrary.org", url: "https://www.carnegielibrary.org/apply/", target: "Submit application",
              fields: [{ label: "Full name", value: "Ada Lovelace" }, { label: "Email", value: "ada@example.com" }, { label: "Home branch", value: "Squirrel Hill" }, { label: "Date of birth", value: "" }, { label: "Email me about events", value: "no" }] });
            let finish;
            const ended = new Promise((r) => (finish = r));
            window.__agentContinue = async () => {
              send({ kind: "approvalDone", id: "ap1", ok: true });
              send({ kind: "toolResult", id: "s6", ok: true, summary: "Clicked “Submit application” → Application received" });
              send({ kind: "toolCall", id: "save", name: "save_page", args: { format: "pdf" } }); await wait(5);
              send({ kind: "saved", path: "/Users/ada/Downloads/BYTE/Application received.pdf", name: "Application received.pdf", format: "pdf", bytes: 184320, url: "https://www.carnegielibrary.org/apply/done" });
              send({ kind: "toolResult", id: "save", ok: true, summary: "Saved Application received.pdf" });
              send({ kind: "browsing", active: false });
              for (const t of ["Done: your library card application went in [2].\n\n", "- **Card number:** it's emailed to ada@example.com within 2 business days\n", "- **Pick up:** bring a photo ID to the **Squirrel Hill** branch to activate it\n\n", "I saved the confirmation page as a PDF in Downloads/BYTE."]) { send({ kind: "content", delta: t }); await wait(10); }
              send({ kind: "done", finishReason: "stop" });
              finish(null);
            };
            return ended;
          }
          if (data.video) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            send({ kind: "toolCall", id: "v0", name: "get_transcript", args: { video: "aircAruvnKk" } }); await wait(10);
            send({ kind: "toolResult", id: "v0", ok: true, summary: "But what is a neural network? · 18:40 · en captions" });
            send({ kind: "toolCall", id: "v1", name: "summarize_video", args: {} }); await wait(10);
            send({ kind: "toolResult", id: "v1", ok: true, summary: "6 chapters" });
            send({ kind: "sources", sources: [{ n: 1, title: "But what is a neural network? (3Blue1Brown)", url: "https://youtu.be/aircAruvnKk", snippet: "", read: true }] });
            send({ kind: "video", id: "aircAruvnKk", title: "But what is a neural network? | Deep learning chapter 1", channel: "3Blue1Brown", seconds: 1120, thumbnail: "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='480' height='270'><rect width='480' height='270' fill='%23101830'/><g fill='%2358c4dc'><circle cx='90' cy='70' r='9'/><circle cx='90' cy='135' r='9'/><circle cx='90' cy='200' r='9'/><circle cx='240' cy='100' r='9'/><circle cx='240' cy='170' r='9'/><circle cx='390' cy='135' r='9'/></g></svg>", language: "en", autoCaptions: false,
              tldr: "A neural network is layers of numbers (neurons) connected by weights and biases; learning means finding the weights that turn pixels into the right digit.",
              keyPoints: [{ start: 52, text: "Recognizing handwritten digits as the running example" }, { start: 297, text: "Each neuron holds a number between 0 and 1 (its activation)" }, { start: 725, text: "Weights and biases: 13,002 knobs to tune" }, { start: 1010, text: "Why the sigmoid is being replaced by ReLU" }],
              chapters: [
                { start: 0, title: "Introduction", summary: "Why recognizing a 3 is easy for people and hard for programs." },
                { start: 172, title: "The structure of the network", summary: "784 input neurons, two hidden layers of 16, and 10 outputs." },
                { start: 403, title: "Why layers?", summary: "The hope that layers pick out edges, then loops and lines, then digits." },
                { start: 646, title: "Edge detection example", summary: "How one neuron's weights could detect an edge." },
                { start: 893, title: "Counting weights and biases", summary: "Learning means finding the right values for all 13,002 of them." },
                { start: 1010, title: "Notes on the sigmoid and ReLU", summary: "Modern networks mostly use ReLU because it trains more easily." },
              ] });
            for (const t of ["> **TL;DR:** A clear, visual introduction to how a neural network turns an image into a digit, and what \"learning\" really means [1].\n\n", "- The running example is handwritten digits [0:52](https://youtu.be/aircAruvnKk?t=52)\n", "- Each neuron is just a number between 0 and 1 [4:57](https://youtu.be/aircAruvnKk?t=297)\n", "- Learning = tuning 13,002 weights and biases [12:05](https://youtu.be/aircAruvnKk?t=725)\n\n", "Great for beginners; no math beyond multiplication is needed."]) { send({ kind: "content", delta: t }); await wait(10); }
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.kitchen) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            const q = args.request.messages[args.request.messages.length - 1].content.toLowerCase();
            if (q.includes("what can i make")) {
              send({ kind: "toolCall", id: "k0", name: "recipe_ideas", args: { have: ["eggs", "spinach", "feta"] } }); await wait(10);
              send({ kind: "toolResult", id: "k0", ok: true, summary: "4 ideas" });
              send({ kind: "recipeIdeas", have: ["eggs", "spinach", "feta"], ideas: [
                { title: "Spinach & Feta Frittata", description: "Custardy eggs baked with wilted spinach and salty feta.", minutes: 25, missing: [], emoji: "🍳" },
                { title: "Shakshuka with Feta", description: "Eggs poached in spiced tomato sauce, finished with feta.", minutes: 30, missing: ["canned tomatoes", "onion"], emoji: "🍅" },
                { title: "Spanakopita Hand Pies", description: "Flaky phyllo parcels of spinach and feta.", minutes: 45, missing: ["phyllo dough"], emoji: "🥟" },
                { title: "Greek Egg Scramble", description: "Soft scrambled eggs with spinach, feta and dill.", minutes: 10, missing: [], emoji: "🥚" },
              ]});
              send({ kind: "content", delta: "I'd go with the **frittata**: it uses all three and looks impressive with almost no effort. Tap a card for the full recipe." });
            } else if (q.includes("plan")) {
              send({ kind: "toolCall", id: "k1", name: "meal_plan", args: { days: 5 } }); await wait(10);
              send({ kind: "toolResult", id: "k1", ok: true, summary: "5 days, 5 meals" });
              const d = (day, title, minutes, emoji) => ({ day, meals: [{ meal: "Dinner", title, description: "", minutes, emoji }] });
              send({ kind: "mealPlan", have: ["chicken thighs", "rice"], days: [d("Monday", "Honey-garlic chicken & rice", 35, "🍗"), d("Tuesday", "Beef & broccoli stir-fry", 25, "🥦"), d("Wednesday", "Sheet-pan salmon & potatoes", 30, "🐟"), d("Thursday", "Chicken fried rice", 20, "🍳"), d("Friday", "Homemade margherita pizza", 40, "🍕")],
                grocery: [{ aisle: "Produce", items: ["2 heads broccoli", "1 lb baby potatoes", "1 head garlic", "Fresh basil"] }, { aisle: "Meat & fish", items: ["1 lb flank steak", "2 salmon fillets"] }, { aisle: "Dairy & eggs", items: ["8 oz fresh mozzarella", "6 eggs"] }, { aisle: "Pantry", items: ["Honey", "Soy sauce", "Pizza dough"] }] });
              send({ kind: "content", delta: "Tuesday and Thursday are the fastest nights. **Prep tip:** cook a double batch of rice on Monday; day-old rice makes the best fried rice on Thursday." });
            } else {
              send({ kind: "toolCall", id: "k2", name: "web_search", args: { query: "spinach feta frittata recipe" } }); await wait(10);
              send({ kind: "toolResult", id: "k2", ok: true, summary: "8 results" });
              send({ kind: "toolCall", id: "k3", name: "read_page", args: { url: "https://www.bbcgoodfood.com/recipes/spinach-feta-frittata" } }); await wait(10);
              send({ kind: "toolResult", id: "k3", ok: true, summary: "recipe: Spinach & feta frittata" });
              send({ kind: "toolCall", id: "k4", name: "write_recipe", args: { dish: "spinach feta frittata" } }); await wait(10);
              send({ kind: "toolResult", id: "k4", ok: true, summary: "6 ingredients, 5 steps" });
              send({ kind: "sources", sources: [{ n: 1, title: "Spinach & feta frittata", url: "https://www.bbcgoodfood.com/recipes/spinach-feta-frittata", snippet: "", read: true }] });
              send({ kind: "recipe", ...{ title: "Spinach & Feta Frittata", description: "Custardy in the middle, golden on top, and on the table in 25 minutes.", category: "breakfast", cuisine: "Greek", servings: 4, prepMin: 10, cookMin: 15, difficulty: "Easy",
  equipment: ["10-inch oven-safe nonstick skillet", "Whisk"],
  ingredients: [
    { qty: 8, unit: "", item: "large eggs", note: "room temperature", have: true },
    { qty: 0.33, unit: "cup", item: "whole milk", note: "80 ml", have: false },
    { qty: 1, unit: "tbsp", item: "olive oil", note: "15 ml", have: false },
    { qty: 5, unit: "oz", item: "baby spinach", note: "140 g", have: true },
    { qty: 0.75, unit: "cup", item: "crumbled feta", note: "100 g", have: true },
    { qty: 0.5, unit: "tsp", item: "kosher salt", note: "", have: false },
  ],
  steps: [
    { text: "Heat the oven to 400°F (200°C) with a rack in the upper third.", minutes: null, cue: "" },
    { text: "Whisk the eggs, milk and salt until no streaks of white remain.", minutes: 1, cue: "Evenly pale yellow and a little frothy" },
    { text: "Warm the oil over medium heat and wilt the spinach, stirring.", minutes: 2, cue: "Bright green and collapsed; no water pooling" },
    { text: "Pour in the eggs, scatter the feta, and cook without stirring until the edges set.", minutes: 3, cue: "Edges pale and firm, center still liquid" },
    { text: "Bake until just set, then rest in the pan.", minutes: 10, cue: "The center jiggles only slightly when you shake the pan" },
  ],
  tips: ["Take it out while the middle still wobbles: it finishes cooking as it rests.", "Squeeze the wilted spinach dry or the frittata turns watery."],
  substitutions: ["Goat cheese or ricotta for feta", "Kale for spinach (cook 2 minutes longer)"],
  storage: "Covered in the fridge up to 3 days; eat cold or reheat gently.",
  image: "", sourceUrl: "https://www.bbcgoodfood.com/recipes/spinach-feta-frittata", sourceName: "bbcgoodfood.com", emoji: "🍳" } });
              send({ kind: "content", delta: "This version adds a splash of milk for a softer set and starts it on the stove so the bottom browns [1]. The one thing to get right: **pull it while the center still wobbles**. Serve with a lemony arugula salad." });
            }
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.places) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            send({ kind: "toolCall", id: "p0", name: "find_places", args: { what: "coffee", near: "Pittsburgh, PA" } });
            await wait(20);
            send({ kind: "toolResult", id: "p0", ok: true, summary: "8 places, 3 open now" });
            const spot = (name, kind, cuisine, address, d, hours, openNow, website = "") => ({ name, kind, cuisine, address, lat: 40.44, lon: -79.99, distanceM: d, hours, openNow, website, phone: "", osmUrl: `https://www.openstreetmap.org/node/${d}` });
            send({ kind: "places", near: "Pittsburgh, Pennsylvania, United States", what: "coffee shops", imperial: true, spots: [
              spot("Rock'n Joe", "cafe", "coffee shop", "524 Penn Avenue, Pittsburgh", 320, "Mo-Fr 07:00-15:00; Sa,Su 07:00-15:00", true, "https://www.rocknjoe.com/"),
              spot("Crazy Mocha", "cafe", "coffee shop", "", 480, "Mo-Fr 06:00-18:00", true, "https://crazymocha.com/"),
              spot("Fernando's Cafe", "cafe", "", "963 Liberty Avenue, Pittsburgh", 520, "", null, "http://fernandoscafe.com/"),
              spot("Buon Giorno Cafe", "cafe", "", "", 610, "Mo-Fr 07:00-14:00", false, "https://buongiorno-eats.com/"),
              spot("De Fer Coffee & Tea", "cafe", "coffee shop", "733 Penn Avenue, Pittsburgh", 700, "Mo-Su 07:00-18:00", true),
              spot("Starbucks", "cafe", "coffee shop", "606 6th Street, Pittsburgh", 820, "", null, "https://www.starbucks.com/"),
            ]});
            send({ kind: "sources", sources: [
              { n: 1, title: "Rock'n Joe", url: "https://www.openstreetmap.org/node/320", snippet: "", read: true },
              { n: 2, title: "Crazy Mocha", url: "https://www.openstreetmap.org/node/480", snippet: "", read: true },
              { n: 5, title: "De Fer Coffee & Tea", url: "https://www.openstreetmap.org/node/700", snippet: "", read: true },
            ]});
            for (const t of ["Three good ones are open right now, all within a short walk:\n\n", "1. **Rock'n Joe**, 0.2 mi, on Penn Avenue, open until 3 pm [1]\n", "2. **Crazy Mocha**, 0.3 mi, open until 6 pm [2]\n", "3. **De Fer Coffee & Tea**, 0.4 mi, open until 6 pm [5]\n\n", "> **Note:** Hours come from OpenStreetMap and may be out of date.\n"]) { send({ kind: "content", delta: t }); await wait(10); }
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.trip) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            for (const [id, name, a, summary] of [
              ["t0", "plan_trip", {}, "3 days in Lisbon, Portugal"],
              ["t1", "trip_weather", { place: "Lisbon, Portugal" }, "Typical weather (May 10 to May 12, last year)"],
              ["t2", "web_search", { query: "best things to do in Lisbon" }, "8 results"],
              ["t3", "web_search", { query: "where to stay in Lisbon neighborhoods" }, "8 results"],
              ["t4", "find_places", { what: "sights", near: "Lisbon, Portugal" }, "12 sights on the map"],
              ["t5", "rank_passages", {}, "21 passages from 9 sources"],
              ["t6", "write_itinerary", {}, "3 days, 11 stops"],
            ]) { send({ kind: "toolCall", id, name, args: a }); await wait(8); send({ kind: "toolResult", id, ok: true, summary }); }
            send({ kind: "sources", sources: [
              { n: 1, title: "The best things to do in Lisbon", url: "https://www.lonelyplanet.com/portugal/lisbon", snippet: "", read: true },
              { n: 2, title: "Castelo de São Jorge", url: "https://castelodesaojorge.pt/en/", snippet: "", read: true },
              { n: 3, title: "Where to stay in Lisbon", url: "https://www.cntraveler.com/lisbon-neighborhoods", snippet: "", read: true },
            ]});
            const it = (time, title, place, note, cost, sources = []) => ({ time, title, place, note, cost, sources });
            send({ kind: "trip", destination: "Lisbon, Portugal", currency: "USD", budget: 1500, travelers: 2, month: null,
              weather: "Typical weather (May 10 to May 12, last year): lows around 57°F, highs up to 75°F, 1 of 3 days with rain.",
              days: [
                { title: "Alfama and the castle", date: "2027-05-10", items: [it("09:00", "Castelo de São Jorge", "Alfama", "Go at opening to beat the lines", 32, [2]), it("12:30", "Lunch at a tasca", "Alfama", "Try grilled sardines", 40, [1]), it("Afternoon", "Tram 28 to Graça", "Graça", "Board at Martim Moniz for a seat", 7, [1]), it("Evening", "Fado dinner", "Alfama", "Book a day ahead", 110, [1])] },
                { title: "Belém", date: "2027-05-11", items: [it("09:30", "Jerónimos Monastery", "Belém", "Buy tickets online", 36, [1]), it("11:30", "Pastéis de Belém", "Belém", "The original custard tarts", 10), it("Afternoon", "MAAT and the riverside", "Belém", "", 24, [1])] },
                { title: "Chiado and Bairro Alto", date: "2027-05-12", items: [it("10:00", "Livraria Bertrand", "Chiado", "World's oldest bookshop", null), it("Afternoon", "Miradouro de São Pedro de Alcântara", "Bairro Alto", "Best at sunset", null, [1]), it("Evening", "Time Out Market", "Cais do Sodré", "", 50, [1])] },
              ],
              costs: [{ category: "Lodging (3 nights)", amount: 540 }, { category: "Food", amount: 330 }, { category: "Sights and tickets", amount: 140 }, { category: "Transport", amount: 60 }],
              packing: ["Comfortable walking shoes (steep hills)", "Light jacket for evenings", "Sunscreen and sunglasses", "Umbrella", "Power adapter (type F)", "Reusable water bottle"],
              tips: ["Get a Viva Viagem card for trams and the metro", "Book the castle and Jerónimos online to skip lines"],
            });
            for (const t of ["> **TL;DR:** Stay in **Chiado or Baixa** so most sights are a walk away; book the castle, Jerónimos and a fado dinner ahead [1][3].\n\n", "## Where to stay\nChiado and Baixa are central and flat by Lisbon standards [3].\n\n", "**Confidence:** Likely — two travel guides agree; prices are estimates.\n"]) { send({ kind: "content", delta: t }); await wait(10); }
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.factCheck) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            for (const [id, name, a, summary] of [
              ["f0", "extract_claims", {}, "1 claim to check"],
              ["f1", "web_search", { query: "we only use 10 percent of our brain" }, "8 results"],
              ["f2", "web_search", { query: "we only use 10 percent of our brain myth OR false OR debunked" }, "8 results"],
              ["f3", "read_page", { url: "https://www.scientificamerican.com/article/do-people-only-use-10-percent-of-their-brains/" }, "Scientific American"],
              ["f4", "read_page", { url: "https://www.britannica.com/story/do-we-really-use-only-10-percent-of-our-brain" }, "Britannica"],
              ["f5", "rank_passages", {}, "9 passages from 5 sources"],
            ]) { send({ kind: "toolCall", id, name, args: a }); await wait(10); send({ kind: "toolResult", id, ok: true, summary }); }
            send({ kind: "sources", sources: [
              { n: 1, title: "Do People Only Use 10 Percent of Their Brains?", url: "https://www.scientificamerican.com/article/do-people-only-use-10-percent-of-their-brains/", snippet: "", read: true },
              { n: 2, title: "Do We Really Use Only 10 Percent of Our Brain?", url: "https://www.britannica.com/story/do-we-really-use-only-10-percent-of-our-brain", snippet: "", read: true },
            ]});
            for (const c of [
              "> **TL;DR:** False. Brain scans show we use virtually all of our brain, just not every part at the same moment [1][2].\n\n",
              "| Claim | Verdict | Evidence |\n|---|---|---|\n",
              "| We only use 10% of our brains | **False** | “It turns out though, that we use virtually every part of the brain, and that most of the brain is active almost all the time.” [1] |\n\n",
              "The myth may come from early 1900s misreadings of brain research; imaging shows even simple tasks light up many regions [2].\n\n",
              "**Confidence:** Verified — several independent science sources agree.\n",
            ]) { send({ kind: "content", delta: c }); await wait(15); }
            send({ kind: "stats", promptTokens: 4100, completionTokens: 150, tokensPerSecond: 20.1, promptMs: 5100, totalMs: 12000, thinkingMs: 0 });
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.compare) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            for (const [id, name, a, summary] of [
              ["d0", "plan_comparison", {}, "2 options, 5 criteria"],
              ["d1", "web_search", { query: "MacBook Air M4 review battery life price" }, "8 results"],
              ["d2", "web_search", { query: "Dell XPS 13 review battery life price" }, "8 results"],
              ["d3", "web_search", { query: "MacBook Air M4 vs Dell XPS 13" }, "8 results"],
              ["d4", "rank_passages", {}, "18 passages from 7 sources"],
              ["d5", "score_options", {}, "10 of 10 scores"],
            ]) { send({ kind: "toolCall", id, name, args: a }); await wait(10); send({ kind: "toolResult", id, ok: true, summary }); }
            send({ kind: "sources", sources: [
              { n: 1, title: "MacBook Air M4 review", url: "https://www.theverge.com/macbook-air-m4-review", snippet: "", read: true },
              { n: 2, title: "Dell XPS 13 (2026) review", url: "https://www.pcmag.com/reviews/dell-xps-13-2026", snippet: "", read: true },
              { n: 3, title: "Best laptops for college students", url: "https://www.nytimes.com/wirecutter/reviews/best-laptops-for-college/", snippet: "", read: true },
            ]});
            const c = (score, reason, sources) => ({ score, reason, sources });
            send({ kind: "decision",
              options: ["MacBook Air M4", "Dell XPS 13"],
              criteria: [{ name: "Battery life", weight: 5 }, { name: "Price", weight: 4 }, { name: "Performance", weight: 3 }, { name: "Portability", weight: 3 }, { name: "Windows apps for class", weight: 2 }],
              scores: [
                [c(9, "About 18 hours in tests", [1]), c(7, "$999, often $899 for students", [1, 3]), c(9, "M4 is fast and silent", [1]), c(9, "2.7 lb", [1]), c(5, "Some course software is Windows-only", [3])],
                [c(7, "About 12 hours", [2]), c(6, "$1,099 as tested", [2]), c(8, "Snapdragon X is quick", [2]), c(9, "2.6 lb", [2]), c(10, "Runs everything a class needs", [3])],
              ],
            });
            for (const t of [
              "> **TL;DR:** For most students the **MacBook Air M4** is the better pick: longer battery life and a lower student price [1][3]. Choose the XPS 13 if your courses require Windows-only software [3].\n\n",
              "## MacBook Air M4\n- All-day battery, about 18 hours [1]\n- Often $899 with education pricing [3]\n\n",
              "## Dell XPS 13\n- Runs Windows course software without workarounds [3]\n- Shorter battery life, about 12 hours [2]\n\n",
              "**Confidence:** Likely — two reviews and a buying guide agree; prices change often.\n",
            ]) { send({ kind: "content", delta: t }); await wait(15); }
            send({ kind: "stats", promptTokens: 5200, completionTokens: 210, tokensPerSecond: 19.2, promptMs: 6100, totalMs: 17000, thinkingMs: 0 });
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.research) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            const steps = [
              ["r0", "plan_research", { question: "What does research say about intermittent fasting for weight loss?" }, "4 searches + papers"],
              ["r1", "web_search", { query: "intermittent fasting weight loss" }, "8 results"],
              ["r2", "web_search", { query: "time restricted eating randomized trial results" }, "8 results"],
              ["r3", "web_search", { query: "intermittent fasting vs calorie restriction" }, "8 results"],
              ["r4", "web_search", { query: "intermittent fasting risks muscle loss" }, "8 results"],
              ["r5", "academic_search", { query: "intermittent fasting weight loss" }, "6 papers"],
              ["r6", "read_page", { url: "https://www.nejm.org/doi/full/10.1056/NEJMra1905136" }, "NEJM review"],
              ["r7", "read_page", { url: "https://www.hopkinsmedicine.org/health/wellness-and-prevention/intermittent-fasting" }, "Johns Hopkins"],
              ["r8", "read_page", { url: "https://www.health.harvard.edu/blog/intermittent-fasting" }, "Harvard Health"],
              ["r9", "rank_passages", {}, "24 passages from 11 sources, by meaning"],
            ];
            for (const [id, name, a, summary] of steps) {
              send({ kind: "toolCall", id, name, args: a });
              await wait(15);
              send({ kind: "toolResult", id, ok: true, summary });
            }
            send({ kind: "sources", sources: [
              { n: 1, title: "Effects of Intermittent Fasting on Health, Aging, and Disease", url: "https://www.nejm.org/doi/full/10.1056/NEJMra1905136", snippet: "", read: true },
              { n: 2, title: "Intermittent Fasting: What is it, and how does it work?", url: "https://www.hopkinsmedicine.org/health/wellness-and-prevention/intermittent-fasting", snippet: "", read: true },
              { n: 3, title: "Effect of Intermittent Fasting on Weight Loss in Overweight and Obese Adults: A Systematic Review of Clinical Trials", url: "https://doi.org/10.1002/fsn3.70412", snippet: "", read: true, meta: { authors: ["Maria L. Santos", "J. Chen", "Ahmed Rahman"], year: 2026, venue: "Food Science & Nutrition", doi: "10.1002/fsn3.70412" } },
              { n: 4, title: "Calorie Restriction with or without Time-Restricted Eating in Weight Loss", url: "https://doi.org/10.1056/NEJMoa2114833", snippet: "", read: true, meta: { authors: ["Deying Liu", "Yan Huang", "Chensihan Huang"], year: 2022, venue: "New England Journal of Medicine", doi: "10.1056/NEJMoa2114833" } },
              { n: 5, title: "Intermittent fasting: The positive news continues", url: "https://www.health.harvard.edu/blog/intermittent-fasting", snippet: "", read: true },
            ]});
            for (const c of [
              "> **TL;DR:** Intermittent fasting helps people lose weight, about **3–8% of body weight** over 8–24 weeks, but trials find it works about as well as ordinary calorie cutting, not better [1][3][4].\n\n",
              "## What the trials show\n\n",
              "- A 2026 systematic review of clinical trials found consistent weight loss across fasting schedules [3].\n",
              "- A year-long randomized trial found **no extra benefit** from time-restricted eating over the same calorie cut [4].\n\n",
              "## Who it suits\n\n",
              "1. People who find a time window easier than counting calories [2].\n",
              "2. Not advised for people with diabetes on medication, or a history of eating disorders, without a doctor [2][5].\n\n",
              "**Confidence:** Likely — several trials agree on the size of the effect; long-term (over 1 year) evidence is still thin [1][4].\n",
            ]) { send({ kind: "content", delta: c }); await wait(15); }
            send({ kind: "stats", promptTokens: 7400, completionTokens: 320, tokensPerSecond: 19.6, promptMs: 9100, totalMs: 26000, thinkingMs: 0 });
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (data.kbAnswer) {
            send({ kind: "started", thinking: false, model: "qwen3.5-9b:Q6_K" });
            send({ kind: "toolCall", id: "f1", name: "search_my_files", args: { query: "lease water damage leak who pays" } });
            await wait(60);
            send({ kind: "toolResult", id: "f1", ok: true, summary: "4 passages from 2 files" });
            send({ kind: "sources", sources: [
              { n: 1, title: "Lease 2026.pdf (p. 4)", url: "file:///Users/logan/Documents/Lease%202026.pdf#page=4", snippet: "Tenant shall promptly report any leak or water damage to Landlord in writing within 48 hours.", read: true },
              { n: 2, title: "Lease 2026.pdf (p. 5)", url: "file:///Users/logan/Documents/Lease%202026.pdf#page=5", snippet: "Landlord is responsible for repairs to plumbing, roofing and appliances supplied with the unit.", read: true },
              { n: 3, title: "Renters insurance.pdf (p. 2)", url: "file:///Users/logan/Documents/Renters%20insurance.pdf#page=2", snippet: "Personal property damaged by sudden and accidental discharge of water is covered.", read: true },
            ]});
            for (const c of [
              "> **TL;DR:** Your landlord pays to fix the leak itself, but you must report it in writing within 48 hours, or you may have to pay for damage that gets worse [1][2].\n\n",
              "## What your lease says\n\n",
              "1. **Report it fast.** Tell your landlord in writing within 48 hours of noticing a leak [1].\n",
              "2. **Repairs are the landlord's job.** Plumbing, roof and supplied appliances are theirs to fix [2].\n",
              "3. **Your belongings** are covered by your renters insurance for sudden water damage [3].\n",
            ]) { send({ kind: "content", delta: c }); await wait(20); }
            send({ kind: "stats", promptTokens: 3100, completionTokens: 140, tokensPerSecond: 21.4, promptMs: 2400, totalMs: 9000, thinkingMs: 0 });
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          if (args.request.model) {
            send({ kind: "started", thinking: false, model: args.request.model });
            const alt = [
              "> **TL;DR:** A new MacBook Air with the M5 chip came out in March 2026.\n\n",
              "- About **20% faster** than the M4 model.\n",
              "- Same design and battery life.\n",
              "- Starts at **$1,099**.\n",
            ];
            for (const c of alt) { send({ kind: "content", delta: c }); await wait(20); }
            send({ kind: "stats", promptTokens: 2900, completionTokens: 90, tokensPerSecond: 27.8, promptMs: 1900, totalMs: 5100, thinkingMs: 0 });
            send({ kind: "done", finishReason: "stop" });
            return null;
          }
          send({ kind: "started", thinking: false, model: "qwen3-14b" });
          send({ kind: "toolCall", id: "c1", name: "web_search", args: { query: "Apple M5 MacBook Air release" } });
          await wait(80);
          send({ kind: "toolResult", id: "c1", ok: true, summary: "8 results" });
          send({ kind: "sources", sources: [
            { n: 1, title: "Apple unveils MacBook Air with M5", url: "https://www.apple.com/newsroom/2026/03/macbook-air-m5/", snippet: "", read: true },
            { n: 2, title: "M5 MacBook Air review: faster, cooler", url: "https://www.theverge.com/reviews/macbook-air-m5", snippet: "", read: true },
            { n: 3, title: "MacBook Air M5 benchmarks", url: "https://www.macrumors.com/m5-air-benchmarks/", snippet: "", read: false },
            { n: 4, title: "Should you upgrade from M4?", url: "https://arstechnica.com/gadgets/m5-air/", snippet: "", read: false },
            { n: 5, title: "M5 vs M4 comparison", url: "https://9to5mac.com/m5-vs-m4/", snippet: "", read: false },
          ]});
          send({ kind: "toolCall", id: "c2", name: "read_page", args: { url: "https://www.apple.com/newsroom/2026/03/macbook-air-m5/" } });
          await wait(60);
          send({ kind: "toolResult", id: "c2", ok: true, summary: "apple.com" });
          send({ kind: "toolCall", id: "c3", name: "read_page", args: { url: "https://www.theverge.com/reviews/macbook-air-m5" } });
          await wait(60);
          send({ kind: "toolResult", id: "c3", ok: true, summary: "theverge.com" });
          const answer = [
            "> **TL;DR:** The M5 MacBook Air launched in March 2026 with roughly 20% faster CPU performance than the M4 model [1][2].\n\n",
            "## Key changes\n\n",
            "1. **M5 chip** with a faster Neural Engine [1].\n",
            "2. **Battery life** unchanged at up to 18 hours [1].\n",
            "3. **Price** starts at $1,099 [2].\n\n",
            "> **Tip:** If you already have an M4 Air, reviewers say the upgrade is optional [2].\n",
          ];
          send({ kind: "toolCall", id: "r1", name: "remember", args: { note: "Uses a MacBook Air M4 with 16 GB" } });
          send({ kind: "toolResult", id: "r1", ok: true, summary: "Uses a MacBook Air M4 with 16 GB" });
          for (const c of answer) { send({ kind: "content", delta: c }); await wait(30); }
          send({ kind: "stats", promptTokens: 3100, completionTokens: 180, tokensPerSecond: 21.4, promptMs: 4200, totalMs: 12600, thinkingMs: 0, draftTokens: 160, draftAccepted: 131 });
          send({ kind: "done", finishReason: "stop" });
          return null;
        }
        default:
          return null;
      }
    },
  };
}

const browser = await chromium.launch({ executablePath: "/opt/pw-browsers/chromium" });
async function page(onboarded, theme = "midnight", extra = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1240, height: 820 }, deviceScaleFactor: 1, colorScheme: "dark" });
  const p = await ctx.newPage();
  const errors = [];
  p.on("pageerror", (e) => errors.push(e.message));
  p.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await p.addInitScript(initScript, { data: { ...mock(onboarded, theme), ...extra } });
  await p.goto(URL);
  await p.waitForTimeout(400);
  return { p, ctx, errors };
}

const shot = (p, name) => p.screenshot({ path: `${OUT}/${name}.png` });

// Onboarding flow
{
  const { p, ctx, errors } = await page(false);
  await shot(p, "01-welcome");
  await p.getByRole("button", { name: "Get started" }).click();
  await p.waitForTimeout(350);
  await shot(p, "02-mac-check");
  await p.getByRole("button", { name: "Continue" }).click();
  await p.waitForTimeout(350);
  await shot(p, "03-choose-model");
  await p.getByRole("button", { name: /^Download/ }).click();
  await p.waitForTimeout(500);
  await shot(p, "04-downloading");
  console.log("onboarding errors:", errors);
  await ctx.close();
}

// Main app
{
  const { p, ctx, errors } = await page(true);
  await p.waitForTimeout(300);
  await shot(p, "04b-saved-chat");
  await p.locator(".conv-item", { hasText: "Cabinet options" }).click();
  await p.waitForTimeout(300);
  await shot(p, "04d-project-versions");
  await p.getByRole("button", { name: "Previous version" }).first().click();
  await p.waitForTimeout(150);
  console.log("after ◀:", await p.locator(".msg.user .bubble").allTextContents(), await p.locator(".versions span").first().textContent());
  await p.getByRole("button", { name: "Next version" }).first().click();
  await p.waitForTimeout(150);
  console.log("after ▶:", await p.locator(".msg.user .bubble").allTextContents());
  await p.getByRole("button", { name: "Project settings" }).first().click();
  await p.waitForTimeout(200);
  await shot(p, "04e-project-editor");
  await p.getByRole("button", { name: "Cancel" }).click();
  await p.getByLabel("Search all chats").fill("lisbon");
  await p.waitForTimeout(400);
  await shot(p, "04c-search");
  await p.getByLabel("Search all chats").fill("");
  await p.getByRole("button", { name: "New chat (⌘N)" }).click();
  await p.waitForTimeout(200);
  await p.evaluate(() => window.__emit("engine://tune", { model: "qwen3.5-9b:Q6_K", step: 3, total: 11, label: "Trying a longer Speed boost look-ahead", done: false, modelIndex: 2, modelCount: 3 }));
  await p.waitForTimeout(200);
  await shot(p, "04f-tuning");
  await p.evaluate(() => window.__emit("engine://tune", { model: "qwen3.5-9b:Q6_K", step: 4, total: 4, label: "Done", done: true }));
  await p.waitForTimeout(200);
  await shot(p, "05-empty");
  await p.getByRole("button", { name: /What's new/ }).click();
  await p.waitForTimeout(1200);
  await p.getByRole("button", { name: /Searched the web/ }).click();
  await p.waitForTimeout(200);
  await shot(p, "06-chat");
  await p.locator(".memory-suggest").scrollIntoViewIfNeeded();
  await p.waitForTimeout(100);
  await p.locator(".msg.assistant").last().screenshot({ path: `${OUT}/06a-remember.png` });
  await p.getByLabel("Answer with").selectOption("compare");
  await p.getByLabel("Message BYTE").fill("Sum that up for me");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(1500);
  await shot(p, "06b-compare");
  await p.keyboard.press("Meta+Comma");

  await p.getByRole("button", { name: "Models", exact: true }).click();
  await p.waitForTimeout(300);
  await shot(p, "07-settings-models");
  await p.locator(".modal-body").evaluate((el) => (el.scrollTop = 700));
  await p.waitForTimeout(150);
  await shot(p, "07b-settings-models-scrolled");
  await p.getByRole("button", { name: "32 GB", exact: true }).click();
  await p.locator(".modal-body").evaluate((el) => (el.scrollTop = 0));
  await p.waitForTimeout(150);
  await shot(p, "07c-settings-models-32gb");
  await p.getByRole("button", { name: "This Mac", exact: true }).click();
  await p.locator(".details-toggle").first().click();
  await p.waitForTimeout(200);
  await p.locator(".model-details").first().scrollIntoViewIfNeeded();
  await shot(p, "07g-model-details");
  await p.getByRole("button", { name: "Memory & chats", exact: true }).click();
  await p.waitForTimeout(300);
  await shot(p, "07d-settings-memory");
  await p.getByRole("button", { name: "Engine", exact: true }).click();
  await p.waitForTimeout(300);
  await shot(p, "07f-settings-speed");
  await p.getByRole("button", { name: "More", exact: true }).click();
  await p.locator("h4", { hasText: "Research" }).scrollIntoViewIfNeeded();
  await p.waitForTimeout(150);
  await shot(p, "07h-settings-research");
  await p.getByRole("button", { name: "About", exact: true }).click();
  await p.waitForTimeout(300);
  await p.locator(".modal-body").evaluate((el) => (el.scrollTop = 200));
  await shot(p, "07e-settings-profiles");
  await p.locator(".field", { hasText: "Photo helper" }).scrollIntoViewIfNeeded();
  await p.waitForTimeout(150);
  await shot(p, "07i-settings-photo-helper");
  await p.getByRole("button", { name: "Models", exact: true }).click();
  await p.waitForTimeout(300);
  await p.locator("#lab-url").fill("https://huggingface.co/bartowski/Mistral-Nemo-Instruct-2407-GGUF/blob/main/Mistral-Nemo-Instruct-2407-Q4_K_M.gguf");
  await p.getByRole("button", { name: "Check", exact: true }).click();
  await p.waitForTimeout(300);
  await p.locator(".model-lab h4").first().evaluate((el) => { el.scrollIntoView({ block: "start" }); el.closest(".modal-body").scrollTop -= 20; });
  await p.waitForTimeout(150);
  await shot(p, "07j-model-lab");
  await p.getByRole("button", { name: "Engine", exact: true }).click();
  await p.waitForTimeout(400);
  await p.locator(".tuning h4").first().evaluate((el) => { el.scrollIntoView({ block: "start" }); el.closest(".modal-body").scrollTop -= 20; });
  await p.waitForTimeout(150);
  await shot(p, "07k-tuning");
  await p.getByRole("button", { name: "Close settings" }).click();
  await p.waitForTimeout(200);
  await p.getByTitle("Writing studio: rewrite, shorten, expand, tone, grammar").click();
  await p.locator("#writing-text").fill(
    "Hi everyone, I am writing to let you know that the meeting that we had planned for Tuesday afternoon has been moved, because several people on the team said that they would not be able to attend at that time, so the new time for the meeting is now Thursday morning at ten o'clock in the same room as before, and please let me know if that does not work for you.",
  );
  await p.getByRole("button", { name: "Shorten", exact: true }).click();
  await p.waitForTimeout(400);
  await shot(p, "23-writing-studio");
  await p.getByRole("tab", { name: "Write something new" }).click();
  await p.locator(".longform textarea").first().fill("Why sleep matters more than cramming, for high school students");
  await shot(p, "23b-writing-new");
  await p.getByRole("button", { name: "Plan it" }).click();
  await p.waitForTimeout(300);
  await shot(p, "23c-writing-outline");
  await p.getByRole("button", { name: "Write it" }).click();
  await p.waitForTimeout(500);
  await shot(p, "23d-writing-longform");
  await p.getByRole("tab", { name: "Edit your text" }).click();
  await p.getByRole("button", { name: "Close", exact: true }).click();
  await p.waitForTimeout(200);
  await p.getByTitle("Job search: postings, deadlines, interview prep").click();
  await p.waitForTimeout(300);
  await shot(p, "24-jobs");
  await p.getByRole("button", { name: "Close", exact: true }).click();
  await p.waitForTimeout(200);
  await p.getByTitle("Assistants: BYTE set up for one job").click();
  await p.waitForTimeout(300);
  await shot(p, "25-assistants");
  await p.locator(".study-panel button.primary", { hasText: "Chat" }).first().click();
  await p.waitForTimeout(400);
  await shot(p, "25b-assistant-chat");
  await p.getByTitle("Settings (⌘,)").click();
  await p.getByRole("button", { name: "About", exact: true }).click();
  await p.waitForTimeout(200);
  await p.getByRole("button", { name: "Appearance", exact: true }).click();
  await p.waitForTimeout(200);
  await shot(p, "08-settings-appearance");
  await p.getByRole("button", { name: "Paper", exact: true }).click();
  await p.getByRole("button", { name: "Close settings" }).click();
  await p.waitForTimeout(300);
  await shot(p, "09-chat-paper");
  console.log("main errors:", errors);
  await ctx.close();
}
// Cloud mode
{
  const { p, ctx, errors } = await page(true);
  await p.keyboard.press("Meta+Comma");
  await p.getByRole("button", { name: "Cloud", exact: true }).click();
  await p.waitForTimeout(250);
  await shot(p, "10-cloud-connect");
  await p.getByLabel("BYTE cloud API key").fill("byte_test_fake_key_for_screens");
  await p.getByRole("button", { name: /Connect/ }).click();
  await p.waitForTimeout(400);
  await shot(p, "10b-cloud-connected");
  await p.getByRole("button", { name: "Close settings" }).click();
  await p.waitForTimeout(300);
  await shot(p, "10b2-cloud-workspace");
  await p.getByRole("button", { name: /New chat/ }).first().click().catch(() => {});
  await p.waitForTimeout(200);
  await p.getByLabel("Message BYTE").fill("Why do spring tides happen?");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(900);
  await shot(p, "10c-cloud-chat");
  await p.getByRole("button", { name: "Attach" }).click();
  await p.waitForTimeout(300);
  await p.getByRole("button", { name: "Library" }).click();
  await p.waitForTimeout(400);
  await shot(p, "10d-cloud-attach");
  await p.getByRole("button", { name: "Close library" }).click();
  await p.getByLabel("Message BYTE").fill("/");
  await p.waitForTimeout(400);
  await shot(p, "10e-slash-prompts");
  await p.getByLabel("Message BYTE").fill("");
  await p.keyboard.press("Meta+Comma");
  await p.locator(".modal-nav").getByRole("button", { name: "Cloud", exact: true }).click();
  await p.waitForTimeout(300);
  await p.getByRole("tab", { name: "Saved prompts" }).click();
  await p.waitForTimeout(300);
  await p.locator(".modal-body").evaluate((el) => (el.scrollTop = el.scrollHeight));
  await p.waitForTimeout(150);
  await shot(p, "10f-cloud-account");
  await p.getByRole("button", { name: "Close settings" }).click();
  await p.getByTitle(/Documents: PDFs/).click();
  await p.waitForTimeout(250);
  await p.getByRole("button", { name: "Slides", exact: true }).click();
  await p.getByLabel("What the document is about").fill("Solar power for beginners, 10 slides for a high-school class");
  await shot(p, "11-docs-create");
  await p.getByRole("button", { name: /Plan it/ }).click();
  await p.waitForTimeout(700);
  await p.getByRole("button", { name: "Classroom" }).click();
  await shot(p, "11b-docs-outline");
  await p.getByRole("button", { name: /Approve and write/ }).click();
  await p.waitForTimeout(700);
  await p.getByRole("button", { name: "Open" }).click();
  await p.waitForTimeout(800);
  await shot(p, "11c-docs-document");
  await p.keyboard.press("Escape");
  await p.getByRole("button", { name: "Close" }).first().click().catch(() => {});
  await p.getByRole("tab", { name: /Both/ }).click();
  await p.waitForTimeout(200);
  await p.getByLabel("Message BYTE").fill("Is the new MacBook Air worth it if I have an M4?");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(1200);
  await shot(p, "10g-both");
  console.log("cloud errors:", errors);
  await ctx.close();
}
// Local chat with files read on this Mac, and a model that sees photos
{
  const { p, ctx, errors } = await page(true, "midnight", {
    vision: true,
    dialogPaths: ["/Users/logan/Documents/lease-2026.pdf", "/Users/logan/Documents/budget.xlsx", "/Users/logan/Pictures/water-damage.png"],
  });
  await p.getByRole("button", { name: /New chat/ }).first().click().catch(() => {});
  await p.getByLabel("Attach").click();
  await p.waitForTimeout(400);
  await p.getByLabel("Message BYTE").fill("Does my lease say who pays for this kind of damage?");
  await shot(p, "13-local-files");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(1500);
  await shot(p, "13b-local-files-sent");
  await p.keyboard.press("Meta+Comma");
  await p.getByRole("button", { name: "Models", exact: true }).click();
  await p.waitForTimeout(300);
  await p.locator(".vision-row").first().scrollIntoViewIfNeeded();
  await p.waitForTimeout(150);
  await shot(p, "13c-sees-images");
  console.log("files errors:", errors);
  await ctx.close();
}
// Knowledge base: folders in Settings, an answer from your files, the reader
{
  const day = 86400000;
  const { p, ctx, errors } = await page(true, "midnight", {
    kbAnswer: true,
    kb: {
      sources: [
        { id: 1, path: "/Users/logan/Documents", addedAt: Date.now() - 3 * day, lastScan: Date.now() - 4 * 60000, error: null, files: 214, chunks: 3810, embedded: 3810, bytes: 7.9e6 },
        { id: 2, path: "/Users/logan/Notes", addedAt: Date.now() - day, lastScan: Date.now() - 4 * 60000, error: null, files: 96, chunks: 402, embedded: 402, bytes: 0.8e6 },
      ],
      embedKey: "nomic-embed-v1.5:Q8_0",
      embedBytes: 146146432,
      embedInstalled: true,
      embedRunning: false,
    },
  });
  await p.keyboard.press("Meta+Comma");
  await p.getByRole("button", { name: "Knowledge base", exact: true }).click();
  await p.waitForTimeout(300);
  await shot(p, "14-kb-settings");
  await p.keyboard.press("Escape");
  await p.getByRole("button", { name: /New chat/ }).first().click().catch(() => {});
  await p.getByLabel("Message BYTE").fill("What does my lease say about water damage from a leak? Who pays?");
  await shot(p, "14a-my-files-toggle");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(1200);
  await shot(p, "14b-kb-answer");
  await p.locator(".source-card").first().click();
  await p.waitForTimeout(500);
  await shot(p, "14c-reader");
  console.log("kb errors:", errors);
  await ctx.close();
}
// Documents made on this Mac: outline, then the written document
{
  const { p, ctx, errors } = await page(true, "midnight");
  await p.getByTitle(/Documents: PDFs/).click();
  await p.waitForTimeout(400);
  await p.getByLabel("What the document is about").fill("A plan to save for a first home in three years");
  await shot(p, "15-docs-local");
  await p.getByRole("button", { name: /Plan it/ }).click();
  await p.waitForTimeout(400);
  await shot(p, "15b-docs-outline");
  await p.getByRole("button", { name: /Write it/ }).click();
  await p.waitForTimeout(1500);
  await shot(p, "15c-docs-written");
  console.log("docs errors:", errors);
  await ctx.close();
}
// Deep research: planned searches, papers, confidence line, citations
{
  const { p, ctx, errors } = await page(true, "midnight", { research: true });
  await p.getByLabel("Message BYTE").fill("What does research say about intermittent fasting for weight loss?");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(1200);
  await p.locator(".activity-head").last().click();
  await p.waitForTimeout(200);
  await shot(p, "16-deep-research");
  await p.getByRole("button", { name: /Cite/ }).last().click();
  await p.getByRole("button", { name: "MLA", exact: true }).click();
  await p.waitForTimeout(200);
  await p.locator(".cite-menu").scrollIntoViewIfNeeded();
  await shot(p, "16b-cite-menu");
  console.log("research errors:", errors);
  await ctx.close();
}
// Fact-check and compare & decide
{
  const { p, ctx, errors } = await page(true, "midnight", { factCheck: true });
  await p.getByLabel("Message BYTE").fill("Is it true that we only use 10% of our brains?");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(900);
  await shot(p, "17-fact-check");
  console.log("fact-check errors:", errors);
  await ctx.close();
}
{
  const { p, ctx, errors } = await page(true, "midnight", { compare: true });
  await p.getByLabel("Message BYTE").fill("MacBook Air M4 vs Dell XPS 13 for a college student?");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(900);
  await p.locator(".decision").scrollIntoViewIfNeeded();
  await shot(p, "17b-decision");
  // Windows software matters most now: the XPS takes the lead.
  const sliders = p.locator(".decision input[type=range]");
  await sliders.nth(4).fill("5");
  await sliders.nth(0).fill("1");
  await p.waitForTimeout(200);
  await shot(p, "17c-decision-reweighted");
  console.log("compare errors:", errors);
  await ctx.close();
}
// Places nearby and a trip plan
{
  const { p, ctx, errors } = await page(true, "midnight", { places: true });
  await p.getByLabel("Message BYTE").fill("Good coffee near me that's open now?");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(700);
  await p.locator(".places").scrollIntoViewIfNeeded();
  await shot(p, "18-places");
  console.log("places errors:", errors);
  await ctx.close();
}
{
  const { p, ctx, errors } = await page(true, "midnight", { trip: true });
  await p.getByLabel("Message BYTE").fill("Plan 3 days in Lisbon in May for 2 people, $1,500");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(800);
  await p.locator(".trip").scrollIntoViewIfNeeded();
  await shot(p, "18b-trip");
  await p.getByRole("tab", { name: /Budget/ }).click();
  await p.waitForTimeout(150);
  await shot(p, "18c-trip-budget");
  await p.getByRole("tab", { name: /Packing/ }).click();
  await p.locator(".trip-packing input").first().check();
  await shot(p, "18d-trip-packing");
  console.log("trip errors:", errors);
  await ctx.close();
}
// Kitchen: ideas, a recipe card, a meal plan, the recipe box; the web pill
{
  const { p, ctx, errors } = await page(true, "midnight", { kitchen: true });
  await p.getByLabel("Message BYTE").fill("What can I make with eggs, spinach and feta?");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(700);
  await p.locator(".ideas").scrollIntoViewIfNeeded();
  await shot(p, "19-kitchen-ideas");
  await p.locator(".idea-card").first().click();
  await p.waitForTimeout(900);
  await p.locator(".recipe").last().scrollIntoViewIfNeeded();
  await shot(p, "19b-recipe-card");
  await p.locator(".recipe").last().getByRole("button", { name: "Metric", exact: true }).click();
  await p.waitForTimeout(200);
  await shot(p, "19f-recipe-metric");
  await p.locator(".recipe").last().getByRole("button", { name: "US", exact: true }).click();
  await p.locator(".recipe").last().evaluate((el) => el.scrollIntoView({ block: "end" }));
  await p.locator(".step-timer").first().click();
  await p.waitForTimeout(1100);
  await shot(p, "19c-recipe-steps");
  await p.getByRole("button", { name: /Save recipe/ }).last().click();
  await p.waitForTimeout(200);
  await p.getByLabel("Message BYTE").fill("Plan dinners for 5 weekdays, we have chicken thighs and rice");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(800);
  await p.locator(".mealplan").scrollIntoViewIfNeeded();
  await shot(p, "19d-meal-plan");
  await p.getByTitle(/Recipe box/).click();
  await p.waitForTimeout(400);
  await shot(p, "19e-recipe-box");
  await p.keyboard.press("Escape");
  await p.getByRole("button", { name: /Web/ }).first().click();
  await p.waitForTimeout(200);
  await p.locator(".composer").screenshot({ path: `${OUT}/19f-web-always.png` });
  console.log("kitchen errors:", errors);
  await ctx.close();
}
// YouTube summary
{
  const { p, ctx, errors } = await page(true, "midnight", { video: true });
  await p.getByLabel("Message BYTE").fill("Summarize https://www.youtube.com/watch?v=aircAruvnKk");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(800);
  await p.locator(".video").scrollIntoViewIfNeeded();
  await shot(p, "20-video-summary");
  console.log("video errors:", errors);
  await ctx.close();
}
// Web agent: steps, the approval card (waiting), then approved with a saved PDF
{
  const { p, ctx, errors } = await page(true, "midnight", { agent: true });
  await p.getByLabel("Message BYTE").fill("Go to carnegielibrary.org and apply for a library card for Ada Lovelace (ada@example.com), home branch Squirrel Hill");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(700);
  await p.locator(".approval").scrollIntoViewIfNeeded();
  await shot(p, "21-agent-approval");
  await p.getByRole("button", { name: "Submit", exact: true }).click();
  await p.waitForTimeout(700);
  await p.locator(".saved-files").scrollIntoViewIfNeeded();
  await shot(p, "21b-agent-done");
  console.log("agent errors:", errors);
  await ctx.close();
}
// Mac control: a reminder waits for OK, then the done card with Undo
{
  const { p, ctx, errors } = await page(true, "midnight", { mac: true });
  await p.getByLabel("Message BYTE").fill("Remind me to call Mom tomorrow at 3pm");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(600);
  await p.locator(".approval").scrollIntoViewIfNeeded();
  await shot(p, "23-mac-approval");
  await p.getByRole("button", { name: "Do it", exact: true }).click();
  await p.waitForTimeout(600);
  await p.locator(".mac-card").scrollIntoViewIfNeeded();
  await shot(p, "23b-mac-done");
  await p.getByRole("button", { name: "Undo" }).click();
  await p.waitForTimeout(300);
  await shot(p, "23c-mac-undone");
  console.log("mac errors:", errors);
  await ctx.close();
}
// Study: flashcards, a quiz, a study session
{
  const { p, ctx, errors } = await page(true, "midnight", { study: true });
  await p.getByLabel("Message BYTE").fill("Make flashcards about the French Revolution");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(600);
  await p.locator(".flash").nth(2).click();
  await p.locator(".study-card").scrollIntoViewIfNeeded();
  await shot(p, "23-flashcards");
  await p.getByLabel("Message BYTE").fill("Quiz me on the periodic table");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(600);
  const qs = p.locator(".quiz-list > li");
  for (const [i, c] of [[0, 1], [1, 2], [2, 1], [3, 0]]) await qs.nth(i).locator(".quiz-choice").nth(c).click();
  await p.getByRole("button", { name: "Check my answers" }).click();
  await p.waitForTimeout(200);
  await p.locator(".quiz").scrollIntoViewIfNeeded();
  await shot(p, "23b-quiz");
  await p.getByTitle("Study: your flashcard decks").click();
  await p.waitForTimeout(300);
  await shot(p, "23c-study-decks");
  await p.getByRole("button", { name: "Study", exact: true }).first().click();
  await p.waitForTimeout(300);
  await p.keyboard.press(" ");
  await p.waitForTimeout(200);
  await shot(p, "23d-study-session");
  console.log("study errors:", errors);
  await ctx.close();
}
// Reviews, prices, game hints
{
  for (const [name, q] of [["22-reviews", "Reviews of the Sony WH-1000XM6"], ["22b-prices", "What's the cheapest place to buy AirPods Pro 3?"], ["22c-hints", "I'm stuck on the Water Temple in Ocarina of Time"]]) {
    const { p, ctx, errors } = await page(true, "midnight", { shop: true });
    await p.getByLabel("Message BYTE").fill(q);
    await p.keyboard.press("Enter");
    await p.waitForTimeout(600);
    if (name === "22c-hints") await p.getByRole("button", { name: "Show the first hint" }).click();
    await p.locator(".shop-card").scrollIntoViewIfNeeded();
    await shot(p, name);
    console.log(name, "errors:", errors);
    await ctx.close();
  }
}
// A model that didn't load: what's using memory, with Quit buttons
{
  const { p, ctx, errors } = await page(true, "midnight", {
    engineError: "The AI engine didn't start: process exited while loading. Using the most memory right now: Google Chrome (2.4 GB), Slack (0.9 GB), Spotify (0.4 GB), Visual Studio Code (0.4 GB). Quitting them frees about 4.1 GB.",
  });
  await p.waitForTimeout(500);
  await shot(p, "12-memory-helper");
  console.log("memory errors:", errors);
  await ctx.close();
}
await browser.close();
