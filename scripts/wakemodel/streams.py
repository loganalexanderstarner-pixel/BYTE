"""Step 2: build augmented training / test streams (6 s of audio each) with frame labels, from the clips of gen_clips.py.

Label per feature frame: 1 = the "hey byte" phrase just ended (frames 2..8 after its end), 0 = anything else, -1 = ignored (right around the
boundary, and the first second while the running mean settles). Augmentation: speed/pitch, volume, room reverb, background noise of many kinds
(white/pink/brown, mains hum, music-like chords, keyboard clicks, fan), TV-like babble, and a loud "BYTE speaking" signal passed through a
speaker/room-like filter at -12..+6 dB against the person's voice (that is what is left after the echo canceller, in the worst case).
"""
import numpy as np, pickle, os, sys
from fractions import Fraction
from scipy.signal import resample_poly, fftconvolve, butter, sosfilt
sys.path.insert(0, os.path.dirname(__file__))
import feats

SR = 16000
LEN = 6 * SR
IGNORE_FIRST = 100  # frames


def rir(rng):
    rt60 = rng.uniform(0.08, 0.7)
    n = int(rt60 * SR)
    t = np.arange(n) / SR
    h = rng.standard_normal(n) * np.exp(-6.9 * t / rt60)
    h[0] = 1.0 + abs(rng.standard_normal()) * 0.3
    return (h / np.sqrt((h ** 2).sum())).astype(np.float32)


def speed(x, rng):
    f = rng.uniform(0.88, 1.12)
    fr = Fraction(f).limit_denominator(24)
    return resample_poly(x, fr.numerator, fr.denominator).astype(np.float32)


def colored(rng, n, kind):
    w = rng.standard_normal(n).astype(np.float32)
    if kind == "white":
        return w
    spec = np.fft.rfft(w)
    f = np.maximum(np.fft.rfftfreq(n, 1 / SR), 20)
    spec = spec / (f ** (0.5 if kind == "pink" else 1.0))
    return np.fft.irfft(spec, n).astype(np.float32)


def noise(rng, n, babble_pool):
    kind = rng.choice(["white", "pink", "brown", "hum", "music", "clicks", "fan", "babble", "babble", "music", "pink"])
    t = np.arange(n) / SR
    if kind in ("white", "pink", "brown"):
        x = colored(rng, n, kind)
    elif kind == "hum":
        f0 = rng.choice([50.0, 60.0]) * rng.uniform(0.99, 1.01)
        x = sum(rng.uniform(0.2, 1) / k * np.sin(2 * np.pi * f0 * k * t + rng.uniform(0, 6)) for k in range(1, 6)).astype(np.float32)
        x += 0.05 * colored(rng, n, "pink")
    elif kind == "music":
        x = np.zeros(n, np.float32)
        pos = 0
        while pos < n:
            dur = int(rng.uniform(0.25, 1.2) * SR)
            root = rng.choice([110, 147, 196, 262, 330, 392, 440]) * 2 ** (rng.choice([0, 3, 5, 7, 12]) / 12)
            seg = np.zeros(min(dur, n - pos), np.float32)
            tt = np.arange(len(seg)) / SR
            for ratio in [[1, 1.25, 1.5], [1, 1.2, 1.5], [1, 1.5, 2], [1]][rng.integers(4)]:
                for h in (1, 2, 3, 4):
                    seg += (1 / h) * np.sin(2 * np.pi * root * ratio * h * tt + rng.uniform(0, 6))
            seg *= np.exp(-tt * rng.uniform(0.5, 4)) * (1 - np.exp(-tt * 80))
            x[pos:pos + len(seg)] += seg
            pos += int(dur * rng.uniform(0.5, 1.0))
        if rng.random() < 0.5:  # a drum-ish thump now and then
            for p in range(0, n, int(SR * rng.uniform(0.3, 0.6))):
                m = min(4000, n - p)
                x[p:p + m] += 2 * np.sin(2 * np.pi * 70 * np.arange(m) / SR) * np.exp(-np.arange(m) / 900)
    elif kind == "clicks":
        x = 0.02 * colored(rng, n, "pink")
        for _ in range(rng.integers(5, 40)):
            p = rng.integers(0, n - 800)
            x[p:p + 800] += rng.standard_normal(800) * np.exp(-np.arange(800) / rng.uniform(15, 80)) * rng.uniform(0.5, 3)
    elif kind == "fan":
        x = colored(rng, n, "brown") + 0.3 * colored(rng, n, "white") * (1 + np.sin(2 * np.pi * rng.uniform(5, 30) * t))
    else:  # babble: several other voices at once (TV, a room)
        x = np.zeros(n, np.float32)
        for _ in range(rng.integers(2, 6)):
            clip = babble_pool[rng.integers(len(babble_pool))].astype(np.float32)
            clip = np.tile(clip, int(np.ceil(n / len(clip)) + 1))
            o = rng.integers(0, len(clip) - n)
            x += clip[o:o + n] / (np.abs(clip[o:o + n]).max() + 1e-6) * rng.uniform(0.3, 1)
    return x.astype(np.float32), kind


def db(x):
    return 10 ** (x / 20)


def level_set(x, rms_db):
    if len(x) == 0:
        return x
    r = np.sqrt((x ** 2).mean()) + 1e-9
    return x * (db(rms_db) / r)


def speaker_path(x, rng):
    """What a loud speaker plus a room does to a signal before it reaches the mic."""
    sos = butter(2, rng.uniform(150, 400), "high", fs=SR, output="sos")
    y = sosfilt(sos, x)
    if rng.random() < 0.7:
        sos = butter(2, rng.uniform(3500, 7500), "low", fs=SR, output="sos")
        y = sosfilt(sos, y)
    y = np.tanh(y * rng.uniform(1.0, 3.0)) / 1.5
    y = fftconvolve(y, rir(rng))[:len(x)]
    return y.astype(np.float32)


def place(buf, clip, start):
    start = max(0, start)
    end = min(len(buf), start + len(clip))
    if start < end:
        buf[start:end] += clip[:end - start]


def make_stream(rng, pool, kind):
    """kind: 'pos' (phrase + maybe a command), 'neg' (ordinary speech), 'hard' (only look-alike phrases, one after another), 'bg' (only noise / music / BYTE speaking)."""
    f32 = lambda c: c.astype(np.float32)
    voice = np.zeros(LEN, np.float32)
    label = np.zeros(LEN // feats.HOP, np.int8)
    te = None
    vol_db = rng.uniform(-38, -12)   # the person's rms level
    if kind == "pos":
        ph = speed(f32(pool["pos"][rng.integers(len(pool["pos"]))][1]), rng)
        s = int(rng.uniform(1.8, 3.8) * SR)
        if s + len(ph) > LEN - 3200:
            s = max(int(1.6 * SR), LEN - 3200 - len(ph))
        place(voice, level_set(ph, 0) * 1.0, s)
        end = s + len(ph)
        te = (end - feats.WIN) // feats.HOP
        if rng.random() < 0.5 and end + 8000 < LEN:   # a command after the phrase
            cmd = speed(f32(pool["cmd"][rng.integers(len(pool["cmd"]))][1]), rng)
            place(voice, level_set(cmd, 0) * rng.uniform(0.7, 1.2), end + int(rng.uniform(0.05, 0.7) * SR))
        if s > 1.0 * SR and rng.random() < 0.6:       # other talk before the phrase
            key = "neg" if rng.random() < 0.6 else "hard"
            c = speed(f32(pool[key][rng.integers(len(pool[key]))][1]), rng)
            c = c[-max(1, s - int(0.25 * SR)):]
            place(voice, level_set(c, 0) * rng.uniform(0.5, 1.2), max(0, s - len(c) - int(rng.uniform(0.1, 0.4) * SR)))
    elif kind in ("neg", "hard"):
        pos = int(rng.uniform(0, 1.5) * SR)
        while pos < LEN - SR:
            key = "neg" if kind == "neg" else "hard"
            c = speed(f32(pool[key][rng.integers(len(pool[key]))][1]), rng)
            place(voice, level_set(c, 0) * rng.uniform(0.6, 1.2), pos)
            pos += len(c) + int(rng.uniform(0.05, 1.5) * SR)
    if kind != "bg":
        voice = level_set(voice, vol_db)
        if rng.random() < 0.75:
            h = rir(rng)
            voice = fftconvolve(voice, h)[:LEN].astype(np.float32)
        if te is not None:
            label[max(0, te + 2):te + 9] = 1
            label[max(0, te - 3):te + 2] = -1
            label[te + 9:te + 20] = -1
        if kind == "pos":
            # the phrase is level_set together with the rest; keep as is
            pass
    x = voice
    # BYTE speaking over everything: a loud long sentence through a speaker path
    if rng.random() < 0.3 or kind == "bg" and rng.random() < 0.5:
        key = "neg"
        ref = np.zeros(LEN, np.float32)
        pos = -int(rng.uniform(0, 2) * SR)
        while pos < LEN:
            c = speed(f32(pool[key][rng.integers(len(pool[key]))][1]), rng)
            if pos < 0:
                c = c[-pos:]
                pos = 0
            place(ref, level_set(c, 0), pos)
            pos += len(c) + int(rng.uniform(0.1, 0.6) * SR)
        ref = speaker_path(ref, rng)
        base = vol_db if kind != "bg" else rng.uniform(-35, -12)
        x = x + level_set(ref, base + rng.uniform(-12, 6))
    # background noise
    n, nk = noise(rng, LEN, [c[1] for c in pool["neg"][:200]])
    if kind == "bg" or rng.random() < 0.15:
        nl = rng.uniform(-62, -28)
    else:
        nl = vol_db - rng.uniform(0, 30)
        if rng.random() < 0.2:
            nl = vol_db - rng.uniform(-6, 4)  # loud background (TV right next to the person)
    x = x + level_set(n, nl)
    x = x + 1e-4 * rng.standard_normal(LEN).astype(np.float32)  # mic self-noise
    if rng.random() < 0.15:  # a gain stage that clips
        x = np.clip(x * rng.uniform(2, 8), -1, 1)
    # a cheap mic: band limits
    if rng.random() < 0.3:
        sos = butter(2, rng.uniform(4500, 7800), "low", fs=SR, output="sos")
        x = sosfilt(sos, x).astype(np.float32)
    x = np.clip(x, -1, 1)
    f = feats.features(x)
    lab = label[:len(f)].astype(np.int8).copy()
    lab[:IGNORE_FIRST] = -1
    return f.astype(np.float16), lab, x


def _job(args):
    seed, pool_path, kinds = args
    pool = _POOL.get(pool_path)
    if pool is None:
        pool = _POOL[pool_path] = pickle.load(open(pool_path, "rb"))
    rng = np.random.default_rng(seed)
    X, Y = [], []
    for k in kinds:
        f, l, _ = make_stream(rng, pool, k)
        X.append(f); Y.append(l)
    return np.stack(X), np.stack(Y)


_POOL = {}


def build_set(pool_path, n, seed, mix=(0.45, 0.25, 0.15, 0.15), jobs=10, per_job=50):
    from multiprocessing import Pool
    rng = np.random.default_rng(seed)
    kinds = rng.choice(["pos", "neg", "hard", "bg"], size=n, p=list(mix))
    chunks = [(seed * 100003 + i, pool_path, kinds[i * per_job:(i + 1) * per_job]) for i in range((n + per_job - 1) // per_job)]
    with Pool(jobs) as p:
        res = p.map(_job, chunks)
    return np.concatenate([r[0] for r in res]), np.concatenate([r[1] for r in res]), kinds


if __name__ == "__main__":
    data = sys.argv[1]
    split, n, seed = sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
    mix = (0.45, 0.25, 0.15, 0.15) if split == "train" else (0.25, 0.25, 0.25, 0.25)
    X, Y, kinds = build_set(os.path.join(data, f"clips_{'train' if split == 'train' else 'test'}.pkl"), n, seed, mix, jobs=int(os.environ.get("JOBS", "10")))
    np.savez(os.path.join(data, f"streams_{split}_{seed}.npz"), X=X, Y=Y, kinds=kinds)
    print(split, X.shape, "pos frames", int((Y == 1).sum()), "streams by kind", {k: int((kinds == k).sum()) for k in ("pos", "neg", "hard", "bg")})
