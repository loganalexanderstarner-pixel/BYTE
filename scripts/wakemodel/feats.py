"""Log-mel features shared by training and the golden test. The Rust code (src-tauri/src/wakemodel.rs) implements exactly the same maths.

16 kHz mono, frame t covers samples [160 t, 160 t + 400), Hann window (periodic), 512-point FFT, power spectrum, 40 triangular HTK-mel
bands from 60 to 7600 Hz (peak 1.0), x = ln(mel + 1e-7), then a running mean (alpha 0.005, started at the first frame) is subtracted.
"""
import numpy as np

RATE, WIN, HOP, NFFT, NMEL = 16000, 400, 160, 512, 40
FMIN, FMAX = 60.0, 7600.0
FLOOR = 1e-7
ALPHA = 0.005


def hz2mel(f): return 2595.0 * np.log10(1.0 + f / 700.0)
def mel2hz(m): return 700.0 * (10.0 ** (m / 2595.0) - 1.0)


def mel_bank():
    pts = mel2hz(np.linspace(hz2mel(FMIN), hz2mel(FMAX), NMEL + 2))
    bins = np.arange(NFFT // 2 + 1) * RATE / NFFT
    fb = np.zeros((NMEL, NFFT // 2 + 1), np.float32)
    for i in range(NMEL):
        lo, c, hi = pts[i], pts[i + 1], pts[i + 2]
        up = (bins - lo) / (c - lo)
        dn = (hi - bins) / (hi - c)
        fb[i] = np.clip(np.minimum(up, dn), 0, None)
    return fb


_FB = mel_bank()
_WINDOW = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(WIN) / WIN)).astype(np.float32)


def logmel(x):
    """x: float32 samples -> (frames, 40) raw log-mel (no mean subtraction)."""
    x = np.asarray(x, np.float32)
    n = (len(x) - WIN) // HOP + 1
    if n <= 0:
        return np.zeros((0, NMEL), np.float32)
    idx = np.arange(WIN)[None, :] + HOP * np.arange(n)[:, None]
    fr = x[idx] * _WINDOW
    spec = np.abs(np.fft.rfft(fr, NFFT, axis=1)) ** 2
    return np.log(spec.astype(np.float32) @ _FB.T + FLOOR).astype(np.float32)


def cmn(lm):
    """Subtract the running mean (causal), as the streaming detector does."""
    from scipy.signal import lfilter
    out = np.zeros_like(lm)
    if len(lm) < 2:
        return out
    x0 = lm[0]
    m, _ = lfilter([ALPHA], [1.0, -(1.0 - ALPHA)], lm[1:], axis=0, zi=((1.0 - ALPHA) * x0)[None, :])
    out[1:] = lm[1:] - m
    return out


def features(x):
    return cmn(logmel(x))
