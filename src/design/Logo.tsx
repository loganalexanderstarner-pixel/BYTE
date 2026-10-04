/** Which of the eight bits are lit (reading order: top row, then bottom row). */
export const LIT_BITS = [1, 6];

const CELL = 10;
const GAP = 3.5;
const W = 4 * CELL + 3 * GAP;
const H = 2 * CELL + GAP;

/**
 * The BYTE mark: one byte, eight bits as rounded squares in two rows of four,
 * two of them lit. Drawn in the live theme's accent so it recolours with the theme.
 * `size` is the height of the square box the mark sits in; the mark is wider than tall.
 */
export function Logo({ size = 28, glow = true }: { size?: number; glow?: boolean }) {
  const height = size * 0.62;
  return (
    <svg
      width={(height * W) / H}
      height={height}
      viewBox={`0 0 ${W} ${H}`}
      className={glow ? "logo-glow" : undefined}
      role="img"
      aria-label="BYTE"
    >
      {Array.from({ length: 8 }, (_, i) => {
        const lit = LIT_BITS.includes(i);
        return (
          <rect
            key={i}
            x={(i % 4) * (CELL + GAP)}
            y={Math.floor(i / 4) * (CELL + GAP)}
            width={CELL}
            height={CELL}
            rx={2.6}
            fill="var(--accent)"
            fillOpacity={lit ? 1 : 0.32}
          />
        );
      })}
    </svg>
  );
}
