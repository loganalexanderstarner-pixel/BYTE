#!/usr/bin/env python3
"""Writes src-tauri/assets/wakemodel_golden.json: scores of the trained network (PyTorch, unfolded batch norm) on a fixed test signal,
so the Rust implementation (own FFT, folded weights, frame-by-frame) can be checked against it.
Usage: python golden.py MODEL.pt OUT.json"""
import sys, os, json
import numpy as np, torch
sys.path.insert(0, os.path.dirname(__file__))
import train, feats


def signal(n=48000):
    i = np.arange(n, dtype=np.float64)
    x = 0.2 * np.sin(0.05 * i) + 0.1 * np.sin(0.9 * i) * (0.5 + 0.5 * np.sin(0.0003 * i)) + 0.02 * np.sin(1.7e-4 * i * i)
    return x.astype(np.float32)


if __name__ == "__main__":
    net = train.Net(); net.load_state_dict(torch.load(sys.argv[1])); net.eval()
    x = signal()
    f = feats.features(x)
    p = train.predict(net, f[None])[0]
    idx = list(range(0, len(p), 20))
    json.dump({"samples": len(x), "frames": len(p), "idx": idx, "scores": [float(p[i]) for i in idx], "feat_sum": [float(f[i].sum()) for i in idx],
               "logmel_first": [float(v) for v in feats.logmel(x)[3][:8]]}, open(sys.argv[2], "w"))
    print("golden:", len(idx), "points")
