#!/usr/bin/env python3
"""Step 3: train the tiny causal "Hey BYTE" detector, measure it on held-out voices, export the weights for Rust.

Model: 1x1 conv 40->C, then 6 residual blocks (causal depthwise conv k=3 with dilations 1,2,4,8,16,32 + 1x1 conv + batch norm + ReLU), then a
linear head -> one score per 10 ms frame. Receptive field 1.27 s. Batch norm is folded into the weights at export.

Usage: python train.py DATA_DIR OUT.bin [epochs]   (env WM_C = width, default 64; WM_THR = threshold written into the file)
"""
import sys, os, json, struct, glob, time
import numpy as np
import torch, torch.nn as nn, torch.nn.functional as F
sys.path.insert(0, os.path.dirname(__file__))
import feats

C = int(os.environ.get("WM_C", "64"))
DIL = [int(v) for v in os.environ.get("WM_DIL", "1,2,4,8,16,32").split(",")]
torch.set_num_threads(int(os.environ.get("TORCH_THREADS", "8")))
DEV = torch.device("cuda" if torch.cuda.is_available() and os.environ.get("WM_CPU") != "1" else "cpu")


class Block(nn.Module):
    def __init__(s, c, d):
        super().__init__()
        s.d = d
        s.dw = nn.Conv1d(c, c, 3, dilation=d, groups=c)
        s.pw = nn.Conv1d(c, c, 1, bias=False)
        s.bn = nn.BatchNorm1d(c)

    def forward(s, x):
        y = s.dw(F.pad(x, (2 * s.d, 0)))
        return x + F.relu(s.bn(s.pw(y)))


class Net(nn.Module):
    def __init__(s, c=C):
        super().__init__()
        s.inp = nn.Conv1d(feats.NMEL, c, 1, bias=False)
        s.bn0 = nn.BatchNorm1d(c)
        s.blocks = nn.ModuleList([Block(c, d) for d in DIL])
        s.head = nn.Conv1d(c, 1, 1)

    def forward(s, x):          # x: (B, T, 40) -> logits (B, T)
        x = F.relu(s.bn0(s.inp(x.transpose(1, 2))))
        for b in s.blocks:
            x = b(x)
        return s.head(x)[:, 0]


def load(data, split):
    X, Y, K = [], [], []
    for f in sorted(glob.glob(os.path.join(data, f"streams_{split}_*.npz"))):
        z = np.load(f)
        X.append(z["X"]); Y.append(z["Y"]); K.append(z["kinds"])
    return np.concatenate(X), np.concatenate(Y), np.concatenate(K)


def triggers(scores, thr, smooth=3, refractory=150, start=100):
    """The decision rule of the Rust detector: mean of the last `smooth` scores >= thr, then silent for `refractory` frames."""
    s = np.convolve(scores, np.ones(smooth) / smooth)[:len(scores)]
    out, last = [], -10 ** 9
    for t in range(start, len(s)):
        if s[t] >= thr and t - last >= refractory:
            out.append(t); last = t
    return out


@torch.no_grad()
def predict(net, X, bs=128):
    net.eval()
    dev = next(net.parameters()).device
    out = []
    for i in range(0, len(X), bs):
        out.append(torch.sigmoid(net(torch.from_numpy(X[i:i + bs].astype(np.float32)).to(dev))).cpu().numpy())
    return np.concatenate(out)


def evaluate(net, X, Y, K, thrs):
    """Stream-level numbers with the decision rule of the detector. Returns {thr: dict} and the hours of audio per kind."""
    P = predict(net, X)
    frames = X.shape[1] - 100
    hours = {k: float((K == k).sum()) * frames * 0.01 / 3600 for k in ("pos", "neg", "hard", "bg")}
    res = {}
    for thr in thrs:
        fa = {"neg": 0, "hard": 0, "bg": 0, "pos_other": 0}
        tp = n_pos = 0
        for i in range(len(X)):
            k = K[i]
            tr = triggers(P[i], thr)
            if k == "pos":
                n_pos += 1
                te = np.where(Y[i] == 1)[0]
                lo, hi = te.min() - 30, te.max() + 60   # a hit anywhere in the last syllable or just after counts: the burst goes to whisper
                tp += any(lo <= t <= hi for t in tr)
                fa["pos_other"] += sum(1 for t in tr if not (lo <= t <= hi))
            else:
                fa[k] += len(tr)
        res[thr] = dict(tpr=tp / max(n_pos, 1), n_pos=n_pos, **fa)
    return res, hours


def fold(conv_w, bn, conv_b=None):
    g = bn.weight.detach() / torch.sqrt(bn.running_var + bn.eps)
    w = conv_w.detach() * g[:, None, None]
    b = bn.bias.detach() - bn.running_mean * g
    if conv_b is not None:
        b = b + conv_b.detach() * g
    return w, b


def export(net, path, thr):
    net.eval().cpu()
    parts = []
    w, b = fold(net.inp.weight, net.bn0)
    parts += [w[:, :, 0].reshape(-1), b]
    for blk in net.blocks:
        dw = blk.dw.weight.detach()[:, 0, :]          # (C, 3)  tap 0 = oldest
        w, b = fold(blk.pw.weight, blk.bn)
        # pw bias comes only from the batch norm; the depthwise bias goes through pw
        b = b + (w[:, :, 0] @ blk.dw.bias.detach())
        parts += [dw.t().reshape(-1), w[:, :, 0].reshape(-1), b]
    parts += [net.head.weight.detach().reshape(-1), net.head.bias.detach().reshape(-1)]
    flat = torch.cat([p.float() for p in parts]).numpy()
    # version 2: weights as 16-bit floats (half the size, the error is far below what the scores need); version 1: 32-bit floats
    half = os.environ.get("WM_F32") != "1"
    flat = flat.astype("<f2" if half else "<f4")
    hdr = b"BYWM" + struct.pack("<IIIIf", 2 if half else 1, C, len(DIL), feats.NMEL, thr) + struct.pack("<%dI" % len(DIL), *DIL)
    open(path, "wb").write(hdr + flat.tobytes())
    return len(hdr) + flat.nbytes


def main():
    data, out = sys.argv[1], sys.argv[2]
    if len(sys.argv) > 3 and sys.argv[3] == "export":      # python train.py DATA OUT.bin export THRESHOLD: re-export OUT.bin.pt with another threshold
        net = Net(); net.load_state_dict(torch.load(out + ".pt", map_location="cpu"))
        print("exported", out, export(net, out, float(sys.argv[4])), "bytes")
        return
    epochs = int(sys.argv[3]) if len(sys.argv) > 3 else 30
    torch.manual_seed(0); np.random.seed(0)
    X, Y, K = load(data, "train")
    Xt, Yt, Kt = load(data, "test")
    print("train", X.shape, "test", Xt.shape, flush=True)
    n = len(X)
    nv = max(200, n // 20)
    Xv, Yv, Kv = X[-nv:], Y[-nv:], K[-nv:]
    X, Y = X[:-nv], Y[:-nv]
    net = Net().to(DEV)
    print(DEV, "params", sum(p.numel() for p in net.parameters()), flush=True)
    opt = torch.optim.AdamW(net.parameters(), 3e-3, weight_decay=1e-3)
    bs = 64
    steps = epochs * (len(X) // bs)
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, 3e-3, total_steps=steps)
    t0 = time.time()
    for ep in range(epochs):
        net.train()
        perm = np.random.permutation(len(X))
        tot = 0
        for i in range(0, len(perm) - bs + 1, bs):
            idx = np.sort(perm[i:i + bs])
            xb = torch.from_numpy(X[idx].astype(np.float32)).to(DEV)
            xb = xb + 0.1 * torch.randn(xb.shape[0], 1, xb.shape[2], device=DEV) + 0.05 * torch.randn_like(xb)   # level offset + feature noise
            # SpecAugment-style: a few mel bands blanked, a short stretch of time blanked (not every sample)
            for b in range(xb.shape[0]):
                if np.random.rand() < 0.5:
                    f0 = np.random.randint(0, feats.NMEL - 4); xb[b, :, f0:f0 + np.random.randint(1, 5)] = 0
                if np.random.rand() < 0.3:
                    m0 = np.random.randint(100, xb.shape[1] - 12); xb[b, m0:m0 + np.random.randint(2, 10)] = 0
            yb = torch.from_numpy(Y[idx].astype(np.float32)).to(DEV)
            logit = net(xb)
            mask = (yb >= 0).float()
            w = mask * (1 + 9 * (yb == 1).float())
            loss = (F.binary_cross_entropy_with_logits(logit, yb.clamp(min=0), reduction="none") * w).sum() / mask.sum()
            opt.zero_grad(); loss.backward(); nn.utils.clip_grad_norm_(net.parameters(), 5.0); opt.step(); sched.step()
            tot += loss.item()
        if ep % 3 == 2 or ep == epochs - 1:
            r, hrs = evaluate(net, Xv, Yv, Kv, [0.6])
            print(f"epoch {ep + 1} loss {tot / (len(perm) // bs):.4f} val {r[0.6]} {time.time() - t0:.0f}s", flush=True)
    net.cpu(); torch.save(net.state_dict(), out + ".pt"); net.to(DEV)
    thrs = [0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95]
    res, hrs = evaluate(net, Xt, Yt, Kt, thrs)
    # Hours of audio that is "not the phrase": ordinary speech, the non-phrase part of the positive streams, look-alikes, noise.
    h_speech = hrs["neg"] + hrs["pos"]
    print(f"TEST (held-out voices and book): {int((Kt == 'pos').sum())} phrase streams; hours scored: speech {h_speech:.2f}, look-alikes {hrs['hard']:.2f}, noise/music/BYTE-speaking {hrs['bg']:.2f}")
    rows = []
    for thr in thrs:
        r = res[thr]
        rows.append(dict(thr=thr, tpr=round(r["tpr"], 3), fa_per_h_speech=round((r["neg"] + r["pos_other"]) / h_speech, 2),
                         fa_per_h_lookalikes=round(r["hard"] / max(hrs["hard"], 1e-9), 1), fa_per_h_noise=round(r["bg"] / max(hrs["bg"], 1e-9), 2)))
        print(rows[-1], flush=True)
    json.dump(dict(hours=hrs, rows=rows), open(out + ".metrics.json", "w"), indent=1)
    thr = float(os.environ.get("WM_THR", "0.6"))
    size = export(net, out, thr)
    print("exported", out, size, "bytes, threshold", thr)


if __name__ == "__main__":
    main()
