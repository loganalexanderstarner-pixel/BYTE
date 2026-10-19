#!/usr/bin/env python3
"""Builds the small real-speech fixture the Rust tests run on (held-out voices, never trained on): src-tauri/assets/wakemodel_fixture.mu
(8-bit mu-law, 16 kHz) and wakemodel_fixture.json (where the phrases end and which parts are not the phrase).

The first held-out clip of each kind is used, in pool order, with no picking of favourites.
Usage: python make_fixture.py DATA_DIR OUT_DIR"""
import sys, os, json, pickle
import numpy as np

data, out = sys.argv[1], sys.argv[2]
pool = pickle.load(open(os.path.join(data, "clips_test.pkl"), "rb"))
rng = np.random.default_rng(3)
SR = 16000


def pick(kind, text=None, n=1, min_len=0.0, max_len=9.0):
    got = [(t, c.astype(np.float32)) for t, c in pool[kind] if (text is None or t == text) and min_len <= len(c) / SR <= max_len]
    return got[:n]


def noise(sec): return (0.002 * rng.standard_normal(int(sec * SR))).astype(np.float32)


parts, marks = [("noise", noise(1.6))], []
def add(kind, label, clip, gain):
    parts.append((kind, noise(0.1))); parts.append((label, clip * gain)); parts.append(("noise", noise(1.4)))

for t, c in pick("pos", "hey byte", 1, min_len=0.6): add("noise", "phrase", c, 0.25)
for t, c in pick("neg", None, 1, min_len=1.5, max_len=4.0): add("noise", "speech", c, 0.25)
for t, c in pick("pos", "okay byte", 1, min_len=0.6): add("noise", "phrase", c, 0.12)
for t, c in pick("hard", "hey siri", 1): add("noise", "other", c, 0.25)
for t, c in pick("neg", None, 1, min_len=1.5, max_len=4.0)[:0]: pass
for t, c in pick("pos", "Hey, BYTE.", 1, min_len=0.6): add("noise", "phrase", c, 0.4)
sig = np.concatenate([p[1] for p in parts]).astype(np.float32)
pos = 0; ends, spans = [], []
for kind, p in parts:
    if kind in ("phrase", "speech", "other"):
        spans.append([kind, pos, pos + len(p)])
        if kind == "phrase": ends.append(pos + len(p))
    pos += len(p)
mu = 255.0
q = np.sign(sig) * np.log1p(mu * np.clip(np.abs(sig), 0, 1)) / np.log1p(mu)
b = np.clip(np.round(q * 127 + 128), 0, 255).astype(np.uint8)
os.makedirs(out, exist_ok=True)
open(os.path.join(out, "wakemodel_fixture.mu"), "wb").write(b.tobytes())
json.dump({"rate": SR, "samples": int(len(sig)), "phrase_ends": ends, "spans": spans}, open(os.path.join(out, "wakemodel_fixture.json"), "w"))
print("fixture", len(sig) / SR, "s,", len(b), "bytes; phrases end at", [round(e / SR, 2) for e in ends])
