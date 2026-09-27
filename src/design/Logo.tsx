import { useId } from "react";

/** The BYTE mark: a neon "B" drawn like a chip, with bit pins on the stem. */
export function Logo({ size = 28, glow = true }: { size?: number; glow?: boolean }) {
  const id = useId().replace(/:/g, "");
  return (
    <svg
      width={size}
      height={size}
      viewBox="230 270 520 484"
      className={glow ? "logo-glow" : undefined}
      role="img"
      aria-label="BYTE"
    >
      <defs>
        <linearGradient id={`n${id}`} gradientUnits="userSpaceOnUse" x1="250" y1="290" x2="720" y2="740">
          <stop offset="0" stopColor="var(--accent)" />
          <stop offset="1" stopColor="var(--accent-2)" />
        </linearGradient>
      </defs>
      <g transform="translate(25 0)" fill="none" stroke={`url(#n${id})`} strokeLinecap="round" strokeLinejoin="round">
        <g strokeWidth={56}>
          <path d="M360 300V724" />
          <path d="M360 300H556a106 106 0 0 1 0 212H360" />
          <path d="M360 512H586a106 106 0 0 1 0 212H360" />
        </g>
        <path strokeWidth={30} d="M250 346H300M250 446H300M250 578H300M250 678H300" />
      </g>
    </svg>
  );
}
