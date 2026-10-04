# What the BYTE app needs from the cluster

This is the app side's wish list for the BYTE cluster (the byte-ai backend at
`byteai.bytebylogan.xyz`). `docs/CLOUD-MODE.md` is the contract for what the
cluster **already does**; this file is what the app **would like it to do next**,
and why.

**How to use it**
- The app side adds a request here when a feature leans on the cluster.
- Whoever works on the cluster picks one up, builds it, and writes the final
  shape into `CLOUD-MODE.md`. The request then moves to **Done** below, with the
  date.
- Nothing here is urgent unless it says so. The app always has a fallback that
  works today, listed under each request, so the cluster can take these in any
  order.

---

## First: this repository is public

Anyone can read this repo and its whole history, including people looking for a
way in. So everything written here, and everything the cluster builds because of
it, has to assume a hostile reader.

**Never put these in this repo**, in this file or any other:
- API keys, tokens, passwords or signing secrets (real or "just for testing").
  The app's tests use obviously fake keys like `byte_test_…`.
- Internal hostnames, IP addresses, ports, or the home network's layout. This
  includes the SearXNG instance, the model servers, the database, and admin
  panels.
- Which machine runs what, GPU/driver versions, or anything else that helps
  someone target the hardware.
- Account names, emails, invite links, or any user's data.
- Admin-only or debug endpoints. If one exists, it isn't documented here.

Describe the **public API** only: the paths a normal key can call, what they take,
and what they return. If a design needs something private to work, write "the
cluster handles this internally" and keep the details off GitHub.

**Every new endpoint must also be secure by itself**, since its shape is public:
- Requires a `byte_` key (401 without one, never a guest mode).
- Scoped to the key's own account: one user can never read, list, delete or bill
  another user's data, even by guessing ids.
- Rate-limited and counted against the account's allowance (429 when spent,
  like the rest of the API).
- Size limits on everything it accepts: prompt length, schema size, file size,
  number of items. It rejects oversize input with a 413 or 422, never by running
  it.
- Model output and user input are untrusted. Never execute them, never fetch
  URLs they name on the server's behalf unless that is the feature (like web
  search), and never let them change another request's settings.
- Error messages say what went wrong for the user ("that schema is too large"),
  not stack traces, file paths or internal addresses.
- Logs keep what's needed to run the service, not people's prompts forever.

---

## Open requests

### 0. For the session on Logan's PC: build the Windows app, then Android, then Linux (not a cluster request)

Logan's Claude session that works on the cluster can also reach his dual-boot PC (Ryzen 7 7800X3D, 32 GB DDR5-6000,
RTX 5080). The owner has asked it to build BYTE's **Windows** app first, then **Linux**. Everything it needs is in
**[`docs/PORTING-WINDOWS-LINUX.md`](PORTING-WINDOWS-LINUX.md)**: the machine, the ground rules (its own branch
`claude/windows-port`, never break the Mac app, no secrets in this public repo), setup, what's Mac-only today, the
feature map (Mac feature → Windows → Linux, and what each can do that the Mac can't), and milestones W1–W4 then
L1–L4. **The rule: every Mac feature, and more** (macOS is the most locked-down; Windows and Linux allow more), on NVIDIA,
AMD and Intel graphics or CPU only. Report to Logan after each milestone. Start with W1: BYTE builds, runs and
answers on the RTX 5080.

**Then Android** (owner, 2026-10-04: Windows → Android → Linux), for Logan's Galaxy Z Fold8 Ultra, built from the
same PC with the phone on adb. Everything is in **[`docs/ANDROID.md`](ANDROID.md)**:
- its branch `claude/android-port` and the same ground rules;
- reading the phone's real specs over adb, and setup (SDK, NDK, JDK 17, Rust target);
- how to reuse the code: Tauri 2 Android, with `llama-server` kept and run from the native library folder;
- using as much RAM as the phone safely can, AI focus with other apps moved to RAM Plus, and an expanded catalog
  for phones up to 16 GB;
- the Mac → Android feature map, and the extras only Android can do;
- signed APKs (Logan makes the keystore and adds the GitHub secrets himself), in-app updates, testing;
- milestones A1–A5.

Start Android when Windows reaches W4, or when Logan says so.


### 1. Structured replies without a conversation (`POST /api/complete`)

**Why.** BYTE's cards (recipes, meal plans, comparisons, trips, reviews, prices,
game hints, YouTube summaries, flashcards, quizzes) are built from
JSON that a model writes to a fixed shape. On a Mac with a local model the
engine enforces that shape. In Cloud mode with no local model (since the "cards everywhere" update), the app
has to fake it: it opens a throwaway conversation, asks for JSON in plain
words, reads the reply forgivingly, then deletes the conversation. That works,
but it costs a conversation per card step (a recipe card is 1–3 steps) and
relies on the model following instructions.

**Suggested shape**

    POST /api/complete
    {
      "system": "You write excellent study flashcards.",
      "prompt": "Write 6 flashcards about photosynthesis …",
      "schema": { …JSON Schema… },
      "mode": "auto",          // optional; same ids as me.modes
      "max_tokens": 1200       // optional; the server caps it
    }
    -> 200 { "json": { …an object matching the schema… } }
    -> 422 { "detail": "…" }   // bad or oversized schema, or the model couldn't fit it

- Grammar-constrained on the server (llama.cpp `json_schema`), so the reply
  always parses.
- Keep the property order as sent (the app relies on it: "question before
  answer" measurably improves small models).
- Creates no conversation, and appears nowhere in the chat list, search or
  export.
- Counts against the account like a `fast`/`auto` message; its own small
  allowance is fine too (see 5).
- Limits: schema ≤ 16 KB, prompt ≤ the model's context, depth/size of the
  schema bounded.

**Until then**, the app uses helper conversations: `cloud/json.rs` (`JsonHelper`).

### 2. Hidden, short-lived conversations

**Why.** It's a smaller step than 1 that removes most of the clutter. When the app
must use a conversation for background work, the user shouldn't see it.

**Suggested shape**
- `POST /api/conversations {"title": "...", "hidden": true}`.
- Hidden conversations are left out of `GET /api/conversations`, conversation
  search and `/api/export`.
- The server deletes them automatically after about 15 minutes.

**Until then**, the app deletes each helper conversation right after use. If that
fails, the user sees a chat called "BYTE card helper".

### 3. Deleting a conversation (confirm `DELETE /api/conversations/{id}`)

**Why.** The app already calls this: for the Cloud workspace's Delete button, and
for cleaning up helper conversations. It isn't in `CLOUD-MODE.md` yet.

**Ask.** Confirm it exists and document it: it returns 204, deletes the
conversation's messages and attachments, and returns 404 for someone else's id
(not 403, so ids can't be probed).

**Until then**, a 404 or 405 is treated as "can't delete here", quietly.

### 4. Cards in the cloud's own answers (a `card` stream event)

**Why.** Today the Mac makes the card and the cloud writes the text. If the cloud
could make cards itself, you'd see them on the phone and on the web too, and the
Mac wouldn't have to make 1–3 extra calls per question.

**Suggested shape.** A new SSE event on the existing stream:

    event: card
    data: { "id": <assistant message id>, "card": { "kind": "recipe", … } }

- The card shapes are the app's `ChatEvent` card types, defined in
  `src/lib/types.ts` (`recipe`, `recipeIdeas`, `mealPlan`, `decision`, `trip`,
  `places`, `video`, `reviews`, `prices`, `hints`, `flashcards`, `quiz`). Keep those field names and the app can show them unchanged.
- Cards are saved on the message, so reopening the chat shows them again.

**Until then**, the app makes the cards itself (on the Mac, or via 1 or the helper
conversations) and sends the cloud its notes.

### 5. Allowance for helper work in `me.budgets`

**Why.** Card steps and photo descriptions are small, background requests. If
they count as full messages, a user could run out of their daily allowance from
a few recipe questions.

**Ask.**
- A separate, smaller budget for requests 1 and 6 (e.g. `"helper_calls"`), shown
  in `GET /api/auth/me` → `budgets` so the app can display it.
- 429 when it's spent. The app then falls back to plain text answers.

### 6. Describe a photo (`POST /api/vision/describe`)

**Why.** Coming soon in the app: when your Mac's model can't see images, a helper
describes the photo so the main model can answer about it. The cluster's MoE is a
vision model, so it could be that helper when a small Mac can't fit a local one.

**Suggested shape**

    POST /api/vision/describe     multipart: file=<image>, question=<optional text>
    -> 200 { "description": "…", "text": "…any text in the image…" }

- Images ≤ 10 MB, JPEG/PNG/HEIC/WebP only.
- The image isn't kept after the reply, unless it was uploaded as an attachment
  on purpose.

**Until then**, the app reads the text in photos on the Mac (Apple Vision). For
anything else, a model that can see is needed.

### 7. Voices (`GET /api/tts/voices`, `POST /api/tts`)

**Why.** BYTE speaks its answers (Talk mode, “Hey BYTE”, Read aloud). On the Mac it uses
free, small voices on the CPU. The cluster's GPU could run bigger, more expressive voices
(emotion, laughs, emphasis) without costing the Mac any memory. The app already has the
client (`cloud/voice.rs`) and a Settings choice: **Made: On this Mac / BYTE Cloud**.

**Suggested shape**

    GET  /api/tts/voices
    -> 200 [{ "id": "aria", "name": "Aria", "about": "Warm, expressive",
              "lang": "en-US", "gender": "female", "expressive": true }, …]

    POST /api/tts   { "text": "…one to three sentences…", "voice": "aria",
                      "style": "calm" | "natural" | "lively", "speed": 1.0 }
    -> 200 audio/wav   (or raw 16-bit little-endian PCM: `audio/L16; rate=24000`)

- The app sends one chunk at a time (the first sentence alone, then about 320 characters)
  while the next is being written, and plays them back to back, so a reply within about a
  second per chunk keeps speech smooth.
- Optional: an `emotion` field later (e.g. "cheerful", "serious"); the app passes `style` today.
- Count it in `me.budgets` like other helper work; 429 when spent.
- 404 means "no voices here": the app then quietly uses the Mac's voice.

**Until then**, the app uses the voice on the Mac (it remembers a 404 for 10 minutes and
doesn't keep asking). Private chats never use cloud voices.

---

## How the app behaves toward the cluster (so the cluster can rely on it)

- **Keys:** stored only in the macOS Keychain, never in the repo, logs or
  settings files. A 401 means "ask the user for a new key".
- **Load:** a slow first token is normal. The app never retries into the queue,
  and backs off on 429 (search: 5 minutes).
- **Outages:** if the cluster can't be reached before it accepted a request, the
  app answers with the local model and says so quietly. After it accepted, the
  app re-reads the saved answer instead of sending it twice.
- **Private chats** never go to the cluster.
- **Unknown fields are ignored and missing ones tolerated**, so the cluster can
  add fields without breaking older app versions. Removing or renaming a field
  needs a note in `CLOUD-MODE.md` first.

---

## Done

Nothing yet. When a request ships, move it here with the date, and put its final
shape in `CLOUD-MODE.md`.
