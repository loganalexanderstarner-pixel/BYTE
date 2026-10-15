/** "4:32" or "1:02:10": time left on a timer, never negative. */
export function leftText(endsMs: number, nowMs: number): string {
  const s = Math.max(0, Math.ceil((endsMs - nowMs) / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(sec)}` : `${m}:${two(sec)}`;
}
