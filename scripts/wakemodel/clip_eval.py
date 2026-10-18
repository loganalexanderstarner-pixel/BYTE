#!/usr/bin/env python3
"""Per-phrase report on held-out clips: each clip alone (1.5 s of room noise in front, quiet level), max smoothed score.
Usage: python clip_eval.py DATA_DIR MODEL.pt [threshold]"""
import sys, os, pickle, collections
import numpy as np, torch
sys.path.insert(0, os.path.dirname(__file__))
import train, feats

data, pt = sys.argv[1], sys.argv[2]
thr = float(sys.argv[3]) if len(sys.argv) > 3 else 0.6
net = train.Net(); net.load_state_dict(torch.load(pt)); net.eval()
pool = pickle.load(open(os.path.join(data, "clips_test.pkl"), "rb"))
rng = np.random.default_rng(0)


def run(clip, gain):
    n = 24000 + len(clip) + 8000
    x = np.concatenate([np.zeros(24000, np.float32), clip.astype(np.float32) * gain, np.zeros(8000, np.float32)]) + 0.002 * rng.standard_normal(n).astype(np.float32)
    p = train.predict(net, feats.features(x)[None])[0]
    return np.convolve(p, np.ones(3) / 3)[:len(p)][100:].max()


by = collections.defaultdict(list)
for gain in (0.15, 0.5):
    for t, c in pool["pos"]: by["POS"].append(run(c, gain))
    for t, c in pool["hard"]: by["LOOKALIKE " + t].append(run(c, gain))
    for t, c in pool["neg"][:600]: by["SPEECH"].append(run(c, gain))
pos, sp = np.array(by["POS"]), np.array(by["SPEECH"])
print(f"threshold {thr}: phrase clips accepted {np.mean(pos > thr):.3f} (n={len(pos)}), ordinary speech sentences wrongly accepted {np.mean(sp > thr):.4f} (n={len(sp)})")
rows = sorted(((k, len(v), float(np.mean(np.array(v) > thr))) for k, v in by.items() if k.startswith("LOOKALIKE")), key=lambda r: -r[2])
for k, n, f in rows[:14]:
    print(f"  {k[11:]:28s} n={n:3d} accepted {f:.2f}")
