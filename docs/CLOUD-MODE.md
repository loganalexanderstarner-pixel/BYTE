# Cloud mode — using BYTE as the backend

The app runs models on the machine it is installed on. This adds a second
option: Logan's own BYTE cluster, reached over the public internet with no
VPN, so the app works anywhere.

**Target is parity with the byte-ai web app**, not just chat.

Base URL: `https://byteai.bytebylogan.xyz`

---

## Authentication

    Authorization: Bearer byte_...

A key authenticates **as its owner** and inherits that account's tier limits
and quota — it is a way in without a browser, not a way around limits.

Users create one at byteai.bytebylogan.xyz → ⚙ Settings → API keys. It is
shown once; only a SHA-256 hash is stored, so it cannot be displayed again.

**Validate the moment it is pasted:** `GET /api/auth/me` → 200 with tier,
modes and budgets, or 401. Fail there, not on the first message.

### The key must never enter this repo

This repository is going public, and history is published with it — a key
committed today stays reachable afterwards. Store it in the **macOS
Keychain** (`keyring` crate, or `security add-generic-password`). Never a
committed config file, a default in source, or a test fixture.

**You do not need a real key to build this.** `GET /api/auth/me` returning
200 vs 401 is the entire contract. Do not ask Logan for one, and decline if
offered.

---

## Modes

`fast`, `auto`, `extended`, `extended_plus` — in that order.

Which a user gets is tier-dependent, so **render only what `me.modes`
returns**. Offering a mode someone cannot use is worse than omitting it.
Do not hardcode the list; it changes per account.

Roughly: `fast` answers immediately with no reasoning; `auto` picks depth per
question; `extended` thinks longer; `extended_plus` is uncapped.

Do **not** choose an engine. A 27B on GPU and a 30B-A3B MoE on CPU sit behind
the same endpoint and the router picks per request based on context length.

---

## Chat

    POST /api/conversations                     -> {id}
    POST /api/conversations/{id}/messages       {"content": "...",
                                                 "attachment_ids": [1,2]}
    GET  /api/conversations                      list
    GET  /api/conversations/{id}                 full history
    POST /api/conversations/{id}/branch          fork from a message

### Streaming

`GET /api/conversations/{id}/stream?since={message_id}` — Server-Sent Events.

| event | payload | meaning |
|---|---|---|
| `message` | full row | a message not yet seen |
| `delta` | `{id, append, status}` | **appended characters only** |
| `status` | `{id, status}` | `streaming` → `done` / `error` |
| `phase` | `{id, phase}` | live progress, e.g. `searching: ...` |
| `sources` | `{id, sources[], grounded}` | pages the answer draws on |
| `done` / `bye` | `{}` | finished / stream closing |

**Use `delta`, never re-render from full rows.** Sending the whole message
each tick cost ~230KB over a 4KB reply.

`sources` arrives *while the answer is still being written* — show them as
they land, not at the end. `phase` is what makes a long turn feel alive
rather than hung.

### Acting on a message

    POST /api/messages/{id}/regenerate     try again (other engine)
    POST /api/messages/{id}/deepen         expand this answer
    POST /api/messages/{id}/justify        explain the reasoning
    POST /api/messages/{id}/stop           stop generation
    POST /api/messages/{id}/answer-now     cut thinking short, answer now
    POST /api/messages/{id}/feedback       thumbs up/down
    DELETE /api/messages/{id}

---

## Photos and files

    POST /api/conversations/{cid}/attachments   multipart -> {id}
    GET  /api/attachments                       the user's library
    GET  /api/attachments/{id}/image            the bytes

Pass `attachment_ids` when posting a message. The library matters: images can
be **reused** without re-uploading, which is most of the value on a slow
connection.

    POST /api/documents/extract-reference       pdf/docx/pptx/txt -> text

Use that to turn an uploaded document into source material for generation
rather than sending the whole file as context.

---

## Documents

    POST /api/jobs/{pdf|pptx|docx|flyer|worksheet|project}   -> {job_id}
    GET  /api/jobs                                            all jobs
    GET  /api/jobs/{id}                                       progress

Generation is a background job — poll it, do not block.

**The approval step is not optional:**

    GET  /api/jobs/{id}/outline     the plan it plans to write
    POST /api/jobs/{id}/approve     {"outline": [...], "library_template": "id"}
    POST /api/jobs/{id}/reject      throw it away, costs nothing

That is where the user edits headings before anything is written. Skipping it
means paying for a document nobody wanted.

    GET /api/documents                        finished documents
    GET /api/documents/{id}/download
    GET /api/documents/{id}/preview           page count
    GET /api/documents/{id}/preview/{page}    page as PNG
    POST /api/documents/{id}/revise           new version from an instruction
    POST /api/documents/{id}/render           same content, another format

`preview` renders pages as images — worth showing before a download so the
user sees what they got.

### Templates

    GET /api/templates?kind=pptx&topic=...    ranked for the topic
    GET /api/templates/{id}/thumb             card image
    GET /api/templates/{id}/preview[/{page}]  every page

Users can build on their own uploaded .pptx/.docx. Offer the picker at the
approval step, with "built-in designs" as the default.

---

## The rest of the web app

    GET/POST/DELETE /api/memories            things BYTE remembers
    GET/POST/DELETE /api/knowledge           uploaded reference material
    GET/POST/DELETE /api/saved-prompts
    GET/POST/DELETE /api/recipes
    GET/POST        /api/settings            theme, locale, default mode
    POST            /api/settings/personal-context
    GET             /api/search/conversations?q=
    GET             /api/export              everything, as a zip

---

## Two things about the backend being someone's house

**Expect queueing.** Under load a request waits rather than failing. A slow
first token is normal; do not treat it as an error or retry into the queue.

**Expect it to be unreachable sometimes.** If the cluster is down, fall back
to local models rather than showing an error. A cloud option backed by
hardware in a house should degrade, not break — that is the honest design.

---

## What is actually behind the endpoint

Worth knowing, because it shapes sensible timeouts, retries and fallbacks.
Deliberately no addresses or topology here — this repository is going public,
and a map of someone's home network does not belong in it. These are
behavioural characteristics, which is what the app needs.

### Two engines, one endpoint

| | dense model | MoE model |
|---|---|---|
| runs on | a discrete GPU | CPU, RAM-backed |
| context | ~33k tokens | **49k per request** |
| concurrency | shared | **4 simultaneous requests** |
| suits | short interactive turns, lower latency | long context, large documents |

**The app must not choose.** A router picks per request based on prompt size:
short turns go to the GPU for latency, long ones to the MoE because its
context is RAM-backed and there is far more RAM than VRAM. Sending a hint
about which engine you want will be ignored, and hardcoding an assumption
about which one answered will break when the router changes.

### Speed, honestly

It is consumer hardware, not a datacentre. Expect **roughly 20–35 tokens per
second** on the MoE — fine to read as it streams, noticeably slower than a
commercial API. First-token latency is usually sub-second but a long prompt
must be processed before generation starts, so a large document can take
tens of seconds before anything appears.

Design implication: **stream, always.** A spinner for twenty seconds reads as
broken; the same twenty seconds with text arriving reads as working. Use the
`phase` event to say what it is doing (`searching: ...`) during the gaps.

### Four concurrent requests, then a queue

Past four, requests queue rather than fail. A slow start is normal under
load — **do not retry into the queue**, that makes it worse. Distinguish
"queued" from "failed" before showing an error.

### It lives in a house

This is the part most worth designing around. The cluster is physical
machines in one person's home, so:

* **power cuts, reboots and upgrades happen.** It is not 99.9%.
* **one machine is a dual-boot gaming PC** that leaves the cluster entirely
  when it boots Windows. Capacity varies by time of day.
* **a redeploy drops in-flight connections.** The work usually continues
  server-side, but the stream dies.

So: **treat unreachable as normal, not exceptional.** If the cluster does not
answer, fall back to local models silently and say so quietly in the UI —
never an error dialog. A cloud option backed by someone's house should
degrade, not break. Retry with backoff, and when a stream dies mid-answer,
reconnect and re-read the conversation rather than assuming the turn was
lost; the answer is usually already saved.

### Accounts and limits

Every key belongs to an account with a tier, and tiers carry real daily
allowances for documents and heavy modes. `GET /api/auth/me` returns the
budgets — **show them**, so a user understands a refusal instead of
experiencing it as a bug. A 429 means an allowance is spent, not that
something is broken.

New users sign up through an invite link; there is no open registration. If
the app is shared, the onboarding needs to account for that: a key cannot be
created without an account, and an account cannot be created without an
invite.
