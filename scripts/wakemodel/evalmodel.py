#!/usr/bin/env python3
"""Held-out numbers for a trained model: python evalmodel.py DATA_DIR MODEL.bin.pt [out.json]

Streams are 6 s of audio built from voices and a book the training never saw. The decision rule is the one the app uses (mean of three
scores >= threshold, then 1.5 s silent). Phrase streams are split by how hard they are:
  clear     no BYTE voice mixed in, voice at least 15 dB above the background
  noisy     no BYTE voice, background less than 15 dB below the voice (TV, music, fan, babble)
  over BYTE BYTE's own voice (speaker path, echo-cancelled residual stand-in) mixed in at -12 .. +6 dB relative to the person
False accepts are counted per hour of audio that is not the phrase.
"""
import sys, os, json, glob
import numpy as np, torch
sys.path.insert(0, os.path.dirname(__file__))
import train

data, pt = sys.argv[1], sys.argv[2]
net = train.Net(); net.load_state_dict(torch.load(pt)); net.eval()
z = np.load(sorted(glob.glob(os.path.join(data, "streams_test_*.npz")))[0])
X, Y, K, M = z["X"], z["Y"], z["kinds"], z["meta"]
P = train.predict(net, X)
frames = X.shape[1] - 100
hrs = {k: float((K == k).sum()) * frames * 0.01 / 3600 for k in ("pos", "neg", "hard", "bg")}
group = np.where(M[:, 0] > 0, "over BYTE", np.where(M[:, 2] >= 15, "clear", "noisy"))
rows = []
for thr in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9):
    tp = {g: 0 for g in ("clear", "noisy", "over BYTE")}; n = dict(tp); gate = dict(tp); fa = {"neg": 0, "hard": 0, "bg": 0, "pos_other": 0}
    for i in range(len(X)):
        tr = train.triggers(P[i], thr)
        if K[i] == "pos":
            te = np.where(Y[i] == 1)[0]; lo, hi = te.min() - 30, te.max() + 60   # a hit anywhere in the last syllable or just after counts: the burst goes to whisper
            n[group[i]] += 1
            tp[group[i]] += any(lo <= t <= hi for t in tr)
            sm = np.convolve(P[i], np.ones(3) / 3)[:len(P[i])]
            gate[group[i]] += sm[max(lo, 100):hi].max() >= thr     # "gate" use: no refractory, the best score near the phrase counts
            fa["pos_other"] += sum(1 for t in tr if not (lo <= t <= hi))
        else:
            fa[K[i]] += len(tr)
    h_speech = hrs["neg"] + hrs["pos"]
    rows.append(dict(thr=thr, tpr_clear=round(tp["clear"] / n["clear"], 3), tpr_noisy=round(tp["noisy"] / n["noisy"], 3), tpr_over_byte=round(tp["over BYTE"] / n["over BYTE"], 3),
                     gate_clear=round(gate["clear"] / n["clear"], 3), gate_noisy=round(gate["noisy"] / n["noisy"], 3), gate_over_byte=round(gate["over BYTE"] / n["over BYTE"], 3),
                     fa_per_h_speech=round((fa["neg"] + fa["pos_other"]) / h_speech, 1), fa_per_h_lookalikes=round(fa["hard"] / hrs["hard"], 1), fa_per_h_noise=round(fa["bg"] / hrs["bg"], 1)))
    print(rows[-1], flush=True)
print("hours scored:", {k: round(v, 2) for k, v in hrs.items()}, "phrase streams per group:", n)
if len(sys.argv) > 4 or len(sys.argv) == 4:
    json.dump(dict(hours=hrs, n=n, rows=rows), open(sys.argv[3], "w"), indent=1)
