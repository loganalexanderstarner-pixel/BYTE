#!/usr/bin/env python3
"""Step 1 of the "Hey BYTE" detector training: synthesise clean speech clips with local text-to-speech voices (piper, many voices).

Writes <out>/clips_<split>.npz-like pickles with 16 kHz float32 arrays:
  pos    "hey byte" style phrases (trimmed so the phrase end is known)
  cmd    short follow-on commands spoken after the phrase ("what is the weather")
  neg    general speech (book sentences) and hard negatives ("hey bite", "hey Siri", "baby", ...)
Voices and book texts are split: the "test" split uses voices and a book the training split never sees.

Usage: python gen_clips.py DATA_DIR   (DATA_DIR holds voices/*.onnx and the three book .txt files, see README in train.py)
"""
import sys, os, re, random, pickle
import numpy as np
from multiprocessing import Pool

DATA = sys.argv[1] if len(sys.argv) > 1 else "/home/node-1/wt2/wakemodel-data"
VOICE_DIR = os.path.join(DATA, "voices")

# The near-misses Whisper writes for the phrase ("hey bite") are accepted by wake.rs, so the detector must accept them too.
POS_PHRASES = ["hey byte", "hey, byte", "hey byte.", "hey Byte!", "okay byte", "okay, byte", "ok byte", "hi byte", "hey byte,", "Hey, BYTE.",
               "hey byte", "hey byte", "okay byte", "hey bite", "hey, bite", "hey bait"]
HARD_NEG = ["hey buy it", "baby", "hey siri", "hey alexa", "hey google", "okay google", "hey kate", "hey mike", "hey bob", "hey buddy",
            "hey bright", "hey light", "hey Pete", "hey bit", "hey bet", "hey there", "hey you", "hey, I bought it", "he buys it", "high five", "hey bud",
            "a bite of food", "bye bye", "good bye", "by the way", "my bike", "hey Dave", "hey Jarvis", "hey computer", "okay, fine", "okay then",
            "hey, what's up", "hey, look at that", "the byte size", "one byte", "a megabyte", "eight bits make a byte", "write it to a byte array",
            "what is a byte", "hey beta", "hey bank", "hey bear", "hey bike", "hey Bryce", "hey Brett", "hey Kyle", "hey Tyler",
            "hey, buy that", "hello", "hey", "okay", "byte", "be quiet", "hi there", "hey, my bad", "I like it", "hey Cortana", "hey Bixby"]
COMMANDS = ["what is the weather", "what time is it", "turn off the lights", "set a timer for ten minutes", "play some music", "tell me a joke",
            "how are you doing today", "what's on my calendar", "remind me to call mom", "open the browser", "read my messages", "what is the capital of france",
            "add milk to my shopping list", "how long is the drive to work", "write an email to john", "summarize this page", "what should I make for dinner",
            "turn the volume down", "stop", "never mind", "can you help me with something", "search for a good pizza place"]
BOOKS = {"train": ["pp.txt", "alice.txt", "holmes.txt", "dracula.txt"], "test": ["frank.txt"]}
# Voices: libritts has 904 speakers; the first 650 train, the last 200 are held out. Two single-speaker voices are held out too.
TRAIN_VOICES = ["en_US-lessac-medium", "en_US-ryan-medium", "en_US-amy-medium", "en_US-joe-medium", "en_US-kristin-medium", "en_GB-jenny_dioco-medium"]
TEST_VOICES = ["en_GB-alan-medium", "en_GB-northern_english_male-medium"]


def sentences(book_files, n, rng):
    text = ""
    for f in book_files:
        raw = open(os.path.join(DATA, f), encoding="utf-8", errors="ignore").read()
        a, b = raw.find("*** START"), raw.rfind("*** END")
        text += raw[a:b] if a > 0 else raw
    text = re.sub(r"\s+", " ", text)
    sents = re.split(r"(?<=[.!?])\s+", text)
    sents = [s.strip(' "_*()[]') for s in sents if 3 <= len(s.split()) <= 24 and not re.search(r"\d|CHAPTER|Project Gutenberg", s)]
    rng.shuffle(sents)
    return sents[:n]


def resample(x, sr):
    if sr == 16000:
        return x.astype(np.float32)
    from scipy.signal import resample_poly
    from math import gcd
    g = gcd(16000, sr)
    return resample_poly(x, 16000 // g, sr // g).astype(np.float32)


def trim(x, thr=0.02, pad=0.03):
    """Cut leading/trailing silence; return (clip, kept) with a short pad."""
    a = np.abs(x)
    idx = np.where(a > thr * max(a.max(), 1e-6))[0]
    if len(idx) == 0:
        return x
    p = int(pad * 16000)
    return x[max(0, idx[0] - p): idx[-1] + p]


def work(job):
    voice, speakers, items, seed = job   # items: list of (kind, text)
    from piper import PiperVoice, SynthesisConfig
    rng = random.Random(seed)
    v = PiperVoice.load(os.path.join(VOICE_DIR, voice + ".onnx"))
    import onnxruntime as ort   # one thread per worker: the pool already uses the cores
    so = ort.SessionOptions()
    so.intra_op_num_threads = 1
    so.inter_op_num_threads = 1
    v.session = ort.InferenceSession(os.path.join(VOICE_DIR, voice + ".onnx"), sess_options=so, providers=["CPUExecutionProvider"])
    out = []
    for kind, text in items:
        sid = rng.choice(speakers) if speakers else None
        cfg = SynthesisConfig(speaker_id=sid, length_scale=rng.uniform(0.75, 1.35), noise_scale=rng.uniform(0.4, 0.9), noise_w_scale=rng.uniform(0.4, 1.0),
                              normalize_audio=True, volume=1.0)
        try:
            ch = [c.audio_float_array for c in v.synthesize(text, cfg)]
            sr = v.config.sample_rate
        except Exception as e:
            continue
        if not ch:
            continue
        x = resample(np.concatenate(ch), sr)
        x = trim(x) if kind in ("pos", "cmd", "hard") else x
        if len(x) < 2000:
            continue
        out.append((kind, text, x.astype(np.float16)))
    return out


def build(split, n_pos, n_cmd, n_hard, n_sent):
    rng = random.Random(1 if split == "train" else 2)
    if split == "train":
        voices = [(v, None) for v in TRAIN_VOICES] + [("en_US-libritts_r-medium", list(range(0, 650)))]
    else:
        voices = [(v, None) for v in TEST_VOICES] + [("en_US-libritts_r-medium", list(range(700, 904)))]
    sents = sentences(BOOKS[split], n_sent, rng)
    items = ([("pos", rng.choice(POS_PHRASES)) for _ in range(n_pos)] + [("cmd", rng.choice(COMMANDS)) for _ in range(n_cmd)] +
             [("hard", rng.choice(HARD_NEG)) for _ in range(n_hard)] + [("neg", s) for s in sents])
    rng.shuffle(items)
    # Spread over voices with extra weight on the multi-speaker one, in chunks so the pool stays busy.
    jobs = []
    chunk = 150
    for i in range(0, len(items), chunk):
        voice, spk = voices[(i // chunk) % len(voices)] if (i // chunk) % 3 else voices[-1]
        jobs.append((voice, spk, items[i:i + chunk], 1000 * (i // chunk) + (0 if split == "train" else 7)))
    res = []
    with Pool(int(os.environ.get("JOBS", "10"))) as p:
        for r in p.imap_unordered(work, jobs):
            res.extend(r)
    d = {k: [(t, x) for kk, t, x in res if kk == k] for k in ("pos", "cmd", "hard", "neg")}
    pickle.dump(d, open(os.path.join(DATA, f"clips_{split}.pkl"), "wb"))
    print(split, {k: (len(v), round(sum(len(x) for _, x in v) / 16000 / 3600, 2)) for k, v in d.items()}, "(count, hours)")


if __name__ == "__main__":
    os.environ.setdefault("OMP_NUM_THREADS", "1")
    build("test", 600, 200, 500, 1300)
    build("train", 7000, 800, 3500, 9000)
