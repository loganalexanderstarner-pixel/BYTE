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

const URL = process.env.URL ?? "http://localhost:4173/";
const OUT = process.env.OUT ?? join(here, "out");
mkdirSync(OUT, { recursive: true });

function mock(onboarded, theme) {
  const GB = 1e9;
  const fit = (f, ctx, note) => ({ fit: f, context: ctx, neededBytes: 11.1 * GB, gpuBudgetBytes: 11.45 * GB, totalRamBytes: 17.18 * GB, note });
  const models = JSON.parse(JSON.stringify(REAL.models));
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
  const settings = { autoTune: true, tuning: onboarded ? { "qwen3.5-9b:Q6_K": { boost: true, kvF16: false, ubatch: 1024, tokensPerSec: 21.4, promptPerSec: 412, chip: "Apple M4 10-core GPU", testedAt: Date.now(), flashAttn: true, draftNMax: 16, draftPMin: 0.75, thorough: true, helperKind: "draft", ngram: true } } : {}, speedBoost: true, speedPref: "balanced", memoryEnabled: true, aboutMe: "I'm Logan. I like clear, practical answers.", loadedAlongside: [], webSearch: true, userName: "Logan", onboardingComplete: onboarded, activeModel: onboarded ? "qwen3.5-9b:Q6_K" : null, contextSize: null, defaultMode: "auto", thinking: "auto", theme, accent: null, fontScale: 1, density: "comfortable", showStats: true };
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
  Object.assign(settings, { cloudConnected: false, cloudBaseUrl: null, cloudAccount: null, useCloud: false, cloudMode: null });
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
        case "engine_status":
          return data.settings.onboardingComplete ? { state: "ready", model: "qwen3.5-9b:Q6_K", context: 16384, boosted: true } : { state: "noModel" };
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
          setTimeout(() => window.__emit("models://download", { kind: "progress", id: args.id, bytes: 3.87e9, total: 9.0e9, bytesPerSec: 48.2e6 }), 50);
          return null;
        case "cloud_status":
          return data.cloud;
        case "cloud_connect":
          data.cloud = { connected: true, baseUrl: "https://byteai.bytebylogan.xyz", account: { name: "Logan", email: "logan@example.com", tier: "Pro", modes: [{ id: "fast", label: "Fast" }, { id: "auto", label: "Auto" }, { id: "extended", label: "Extended" }, { id: "extended_plus", label: "Extended+" }], budgets: { daily_messages: { used: 12, limit: 500 }, documents_left: 40 } } };
          data.settings = { ...data.settings, cloudConnected: true, cloudMode: "auto" };
          return data.cloud;
        case "cloud_action":
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
          return ["/Users/logan/Pictures/tide-pool.jpg"];
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
async function page(onboarded, theme = "neon-night") {
  const ctx = await browser.newContext({ viewport: { width: 1240, height: 820 }, deviceScaleFactor: 1, colorScheme: "dark" });
  const p = await ctx.newPage();
  const errors = [];
  p.on("pageerror", (e) => errors.push(e.message));
  p.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await p.addInitScript(initScript, { data: mock(onboarded, theme) });
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
  await p.getByRole("button", { name: "Memory & chats", exact: true }).click();
  await p.waitForTimeout(300);
  await shot(p, "07d-settings-memory");
  await p.getByRole("button", { name: "Engine", exact: true }).click();
  await p.waitForTimeout(300);
  await shot(p, "07f-settings-speed");
  await p.getByRole("button", { name: "About", exact: true }).click();
  await p.waitForTimeout(300);
  await p.locator(".modal-body").evaluate((el) => (el.scrollTop = 200));
  await shot(p, "07e-settings-profiles");
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
  console.log("cloud errors:", errors);
  await ctx.close();
}
await browser.close();
