// Optional soft sounds (Settings → Appearance), made with WebAudio: no sound files.
let ctx: AudioContext | null = null;

/** A short, quiet two-note chime: "answer ready" (up) or "reminder" (down). */
export function chime(kind: "done" | "reminder" = "done"): void {
  try {
    ctx ??= new AudioContext();
    const notes = kind === "done" ? [660, 880] : [880, 660];
    const t0 = ctx.currentTime;
    notes.forEach((f, i) => {
      const osc = ctx!.createOscillator();
      const gain = ctx!.createGain();
      osc.type = "sine";
      osc.frequency.value = f;
      const at = t0 + i * 0.09;
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(0.06, at + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.22);
      osc.connect(gain).connect(ctx!.destination);
      osc.start(at);
      osc.stop(at + 0.25);
    });
  } catch {
    // No audio (or not allowed yet): stay quiet.
  }
}
