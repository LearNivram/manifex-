// Lightweight SVG charts (no chart library: small bundle, crisp at any DPI).

import { useMemo } from "react";
import { cn } from "@/lib/format";

export interface Series {
  name: string;
  color: string;
  points: { t: number; v: number }[];
}

export function LineChart({
  series,
  height = 160,
  format = (v: number) => String(Math.round(v)),
  max,
  className,
}: {
  series: Series[];
  height?: number;
  format?: (v: number) => string;
  max?: number;
  className?: string;
}) {
  const W = 600;
  const H = height;
  const pad = { l: 44, r: 8, t: 8, b: 18 };
  const { tMin, tMax, vMax } = useMemo(() => {
    const all = series.flatMap((s) => s.points);
    const tMin = Math.min(...all.map((p) => p.t), Date.now() - 60_000);
    const tMax = Math.max(...all.map((p) => p.t), Date.now());
    const vMax = max ?? Math.max(1, ...all.map((p) => p.v)) * 1.15;
    return { tMin, tMax, vMax };
  }, [series, max]);
  const x = (t: number) => pad.l + ((t - tMin) / Math.max(1, tMax - tMin)) * (W - pad.l - pad.r);
  const y = (v: number) => pad.t + (1 - v / vMax) * (H - pad.t - pad.b);
  const ticks = [0, 0.5, 1].map((f) => f * vMax);
  return (
    <div className={cn("flex flex-col gap-2", className)}>
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full" style={{ height }} preserveAspectRatio="none" role="img">
        {ticks.map((t) => (
          <g key={t}>
            <line x1={pad.l} x2={W - pad.r} y1={y(t)} y2={y(t)} stroke="var(--border)" strokeDasharray={t === 0 ? undefined : "3 3"} vectorEffect="non-scaling-stroke" />
            <text x={pad.l - 6} y={y(t) + 3} textAnchor="end" fontSize="10" fill="var(--subtle)">
              {format(t)}
            </text>
          </g>
        ))}
        {series.map((s) => {
          if (s.points.length < 1) return null;
          const d = s.points.map((p, i) => `${i ? "L" : "M"}${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`).join(" ");
          const area = `${d} L${x(s.points.at(-1)!.t).toFixed(1)},${y(0)} L${x(s.points[0].t).toFixed(1)},${y(0)} Z`;
          return (
            <g key={s.name}>
              <path d={area} fill={s.color} opacity={0.08} />
              <path d={d} fill="none" stroke={s.color} strokeWidth={1.6} vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
            </g>
          );
        })}
      </svg>
      <div className="flex flex-wrap gap-3 pl-11 text-[11.5px] text-muted">
        {series.map((s) => (
          <span key={s.name} className="flex items-center gap-1.5">
            <span className="size-2 rounded-full" style={{ background: s.color }} />
            {s.name}
            {s.points.length > 0 && <span className="tabular text-fg">{format(s.points.at(-1)!.v)}</span>}
          </span>
        ))}
      </div>
    </div>
  );
}

/** Stacked horizontal capacity bar: usage / requests / limits against allocatable. */
export function CapacityBar({ usage, requests, limits, allocatable }: { usage?: number | null; requests: number; limits: number; allocatable: number }) {
  const pct = (v: number) => `${Math.min(100, (v / Math.max(allocatable, 1)) * 100)}%`;
  return (
    <div className="relative h-2 w-full overflow-hidden rounded-full bg-hover">
      <div className="absolute inset-y-0 left-0 rounded-full bg-accent/25" style={{ width: pct(requests) }} />
      {usage != null && <div className="absolute inset-y-0 left-0 rounded-full bg-accent" style={{ width: pct(usage) }} />}
      {limits > 0 && <div className="absolute inset-y-0 w-px bg-warn" style={{ left: pct(limits) }} />}
    </div>
  );
}

export function Donut({ segments, size = 72, label }: { segments: { value: number; color: string }[]; size?: number; label?: React.ReactNode }) {
  const total = segments.reduce((a, s) => a + s.value, 0) || 1;
  const r = 15.9155;
  let offset = 25;
  return (
    <div className="relative" style={{ width: size, height: size }}>
      <svg viewBox="0 0 42 42" width={size} height={size}>
        <circle cx="21" cy="21" r={r} fill="none" stroke="var(--hover)" strokeWidth="4" />
        {segments.map((s, i) => {
          const len = (s.value / total) * 100;
          const el = <circle key={i} cx="21" cy="21" r={r} fill="none" stroke={s.color} strokeWidth="4" strokeDasharray={`${len} ${100 - len}`} strokeDashoffset={offset} />;
          offset -= len;
          return s.value > 0 ? el : null;
        })}
      </svg>
      {label && <div className="absolute inset-0 flex items-center justify-center text-[13px] font-semibold">{label}</div>}
    </div>
  );
}
