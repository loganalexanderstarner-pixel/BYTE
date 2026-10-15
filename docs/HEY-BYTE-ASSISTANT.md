# Hey BYTE as a replacement for Alexa

The owner's wish (2026-10-13): say "Hey BYTE" and get what an Alexa gives: "turn the volume down a bit", "what's the
weather in Dover, Tennessee", "open Spotify to my gaming playlist". It should answer **immediately**, use the model on
the PC, BYTE Cloud, or either one (a switch), and have a **voice** to choose, because it will often be used while gaming,
when the PC's graphics card is busy and the cloud is the quick path.

## What exists (checked 2026-10-13)

- Wake phrase listener (`wake.rs`, Mac and Windows), Quick Ask opens with the mic on, speech to text (whisper), Talk mode
  (listen, answer aloud, listen), answers worded for being heard (`commands.rs`, "the answer will be heard").
- Every PC action through the same flows as the chat: volume (number or louder/quieter), mute, dark mode, Wi-Fi, display
  sleep, Settings pages, music play/pause/next/previous/now playing, reminders and to-dos (BYTE Tasks), mail drafts,
  weather (forecast service, any town), calculator, units, news digest, PC check-up, files. A request said after the wake
  phrase has it stripped (`router::without_wake_phrase`).
- Cloud mode: the PC flows run on the PC and the cloud writes the answer (`backend::prepare_for_cloud`). Not yet checked
  against the real cluster.
- Voices: local (Kokoro, Piper and others on the PC) and a Settings choice **Made: On this PC / BYTE Cloud**
  (`cloud/voice.rs`). **The cloud half does not exist on the cluster**: `byte-ai` has no `/api/tts` (the cluster API notes
  request 7 was never built).

## What is missing, in the order to build it

1. **Timers and alarms** ("set a timer for ten minutes", "how long is left", "cancel the timer", "wake me at 7"). **Built
   2026-10-13** (`timers.rs`): second precision, survives a restart, rings with an alarm toast. Still to do: say "your timer
   is done" aloud (the `timer://done` event is sent) and a timers panel.
2. **Open Spotify to a playlist by name.** *Built 2026-10-13 (`playlists.rs`); autoplay not yet verified because nobody is signed
   in to Spotify on a development PC.* Windows music today only searches. Spotify's desktop app plays a playlist from
   `spotify:playlist:<id>:play`, so BYTE keeps a small list of playlist names and links (say it once: "my gaming playlist is
   <share link>"), then "open Spotify to my gaming playlist" plays it. Without a saved name it opens Spotify's search for
   the words. Reading the real playlists from the account (the Spotify Web API) needs the owner to log in once; later.
3. **Hey BYTE settings:** *Engine switch built 2026-10-13 (`busy.rs`, `lib/heyByte.ts`); the voice picker already exists (Settings, Models, Voice) and is empty on the cloud side until step 4.* *Answers from:* This PC / BYTE Cloud / Automatic (cloud while a game is running or the graphics
   card is busy, this PC otherwise), and *Voice:* the same picker with local and cloud voices, with a sample button.
4. **Cloud voices on the cluster:** *Now also an expressive, streaming voice (Qwen3-TTS, first sound 0.2 s, the owner picked the lighter model on 2026-10-13).* *Built and live 2026-10-13 (28 Kokoro voices, 0.3 s a sentence); the app's own client was checked against it over the internet (0.4 s a sentence); not yet heard through a PC's speakers.* `GET /api/tts/voices` and `POST /api/tts` (request 7), a small separate service so it
   does not touch the live byte-ai deploy, streaming a sentence at a time.
5. **Immediate answers.** *Built 2026-10-13: spoken commands answer with no model (PC), and spoken questions go to a small voice model on the cluster (`cloud/fast.rs`; first word 0.35 to 0.9 s). Still to do: the same lane for the PC's own local model, a short sound the instant the wake phrase is heard, and shorter end-of-speech waiting.* Commands that need no model (volume, mute, music, timers, "what time is it", weather) skip the
   model and answer in a few words at once; the rest streams the first sentence into speech while the rest is written,
   in short spoken style. A short sound plays the moment the wake phrase is heard.
6. **More of what an Alexa does:** shopping and other lists, "what time is it", jokes and facts, routines
   ("good night"), later smart-home devices.

## Needs the owner

- Say the playlist links once (or log in to Spotify once, for the account version).
- Nothing else for 1 to 5: they can be built and checked on the PC and on the cluster.


## Where it is going (the owner, 2026-10-13)

- **As capable as Alexa, then more**, and **on the phone** as the replacement for "Hey Google" (the Android app already has BYTE as
  the phone's assistant, the `android` branch). So Hey BYTE is designed as one **shared voice service on the cluster**, not
  something each app rebuilds:
  - **Voice brain:** a small, fast model of its own on spare cluster RAM and CPU (so it never takes from the big engines),
    reached through a short, streaming voice route that skips the slow parts of the normal chat pipeline. Open questions that
    need real knowledge go to the big engines or to a web lookup, then are spoken.
  - **Instant skills with no model at all:** time and date, weather, timers and alarms, math and units, volume and other device
    controls, music, lists. These are code, so they answer in milliseconds on every device (the PC ones exist; Android has its own
    device controls and calls the same service for the rest).
  - **Voice:** the cluster's speech service (`byte-tts`, Kokoro today). The owner wants it to sound like a real person with
    feeling. Kokoro is natural but not very emotional; a model that takes emotion cues (Orpheus, Chatterbox, Sesame CSM and the
    like) needs a GPU, and the only spare GPU memory is about 6 GB on the RTX 4060 Ti. To be settled by listening to the same
    lines in each voice, not by guessing.
  - **Same login and budgets as the rest of BYTE Cloud** (a personal API key), so the phone and the PC are one account.
- **Parity list to work through** (status in `CHANGELOG.md`): timers and alarms (done on PC), reminders and lists, weather
  (done), time and date, math and units, music and playlists (done on PC, unverified), device and PC control (done on PC), phone
  calls and texts and navigation (Android), smart-home devices, news and sports, routines ("good night"), follow-up questions
  without saying the wake phrase again, and several people's voices.
- **Android:** the Android session owns that app. What it needs from here: the cluster voice service (above), the instant-skills
  list, and the wake-phrase and spoken-answer behavior described in this file.

## Reading the person's mood (idea from the owner, tried 2026-10-13)

- **SenseVoice** (sherpa-onnx, `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8`, about 230 MB) gives the words, the language, the
  mood (`HAPPY`, `SAD`, `ANGRY`, `SURPRISED`, `NEUTRAL`, or unknown) and sound events (laughter, cough...) in one pass. On this CPU it
  decoded 4 to 10 seconds of speech in 0.05 to 0.14 s, far faster than Whisper, so it is also the quicker way to hear the question.
  English, Chinese, Japanese, Korean and Cantonese; Whisper stays for everything else.
- First try, on synthetic speech only: the sympathetic and excited lines read as `SAD` and `SURPRISED`; flat speech read as unknown.
  Real voices are not tested yet.
- **How to use it:** only as a soft hint. It tells the voice brain how to answer ("they sound tired: be gentle and short") and tells
  the speech voice how to say it (softer and slower, or brighter). BYTE never announces "you sound sad". The audio stays on the PC;
  only a one-word label would travel to the cluster.
- **More in the same spirit:** match the person's energy (whisper back to a whisper, quiet late at night, short when they sound
  rushed); recognize who is talking (the app already carries speaker models) so the answer has the right context; let the
  person talk over BYTE to stop it; keep listening for a few seconds after an answer so a follow-up needs no wake phrase.

## Personality modes (owner, 2026-10-13)

- **Casual, Direct, Unhinged 18+, Auto** (Settings → About → Personality; `style.rs`, `lib/talkMode.ts`). Casual and Direct are plain
  style rules. **Auto** starts Casual and learns slowly from numbers only (message length, polite or casual, whether they swear, never what
  they wrote); one swear or "f--- you BYTE" as a joke changes nothing, it takes a steady pattern over many messages and several days, it
  only mirrors swearing if "Swear back if I do (18+)" is on, and it never reaches Unhinged by itself.
- **Unhinged** is an 18+ entertainment mode with a warning when it is switched on ("don't take its advice; use it for weather, timers,
  lists and facts"). It swears, roasts, jokes crude, dark, dirty and cartoonishly violent, does innuendo and "that's what she said", and
  twists the person's own words into dirty jokes, all as jokes between adults. The owner decided it needs no limits beyond the warning. What
  stays, and is written into its instructions: nothing sexual involving minors, no serious planning or urging of real violence, break-ins or
  harassment (a joke stays a joke), a real crisis gets kindness, and no explicit sex scenes (jokes only). Off in kids mode.
- **Making the jokes land** (not lame): the instructions carry the craft (only when their words set one up, short, never explained, callbacks to
  what they said earlier, at most about one message in three). Still to do: a few hundred original jokes in these styles, written by the
  big model on the cluster and picked by similarity to what the person just said, as examples. Not scraped from Reddit (their rules, and
  the quality is poor).
- The voice lane takes the same words (`mode_notes` on `/api/voice/ask`), so Hey BYTE talks the same way as the chat.

## Voice profiles ("teach it your voice")

- Cluster side, built and checked 2026-10-13: `POST /api/voiceprints` (a name and a few recordings; more people allowed, up to eight; more
  samples sharpen a voice), `GET` the list, `DELETE` one, and `POST /api/voiceprints/identify` (who is this). Only a 192-number voiceprint is
  kept, never the recordings. Stand-in voices: taught voices were recognized at 0.82 to 0.92 similarity, strangers 0.24 to 0.33; a very similar
  voice from the same speech model reached 0.63, so a lock ("only my voice") uses a stricter bar (0.70) than just recognizing people (0.50).
- App side, built 2026-10-13 (not yet tried with a real microphone): the teach flow, the list with Forget, who-is-speaking on each spoken turn,
  the name used in answers and the lock. Still to do: a per-person Auto style profile, and the same on the phone.

## Friend-like memory (owner, 2026-10-13)

Built and checked on the cluster: automatic learning from what the person says (no "remember that"), notes that update as life changes, a rolling
two-sentence summary, relevance-only recall, per-account privacy (two-account test), off in private chats. Notes are short plain sentences
("Logan prefers dark mode", "Logan's favorite color is yellow (reason unknown)"), they need not be third person. Still to do: the desktop app's own
local memory should use the account's notes when BYTE Cloud is connected (today the local chat and the cloud lane keep separate notes), a screen to
see and edit them next to the voices, per-person notes when several voices are taught, and the same lane on the phone.
