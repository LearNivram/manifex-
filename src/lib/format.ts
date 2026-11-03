// Formatting helpers (pure, unit tested).

export function age(ts?: string | null, now: number = Date.now()): string {
  if (!ts) return "—";
  const t = Date.parse(ts);
  if (Number.isNaN(t)) return "—";
  let s = Math.max(0, Math.floor((now - t) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 && h < 10 ? `${h}h${m % 60}m` : `${h}h`;
  const d = Math.floor(h / 24);
  if (d < 365) return h % 24 && d < 10 ? `${d}d${h % 24}h` : `${d}d`;
  const y = Math.floor(d / 365);
  s = d % 365;
  return s ? `${y}y${s}d` : `${y}y`;
}

export function bytes(n?: number | null, digits = 1): string {
  if (n === undefined || n === null || Number.isNaN(n)) return "—";
  const units = ["B", "Ki", "Mi", "Gi", "Ti", "Pi"];
  let i = 0;
  let v = n;
  while (Math.abs(v) >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : digits)} ${units[i]}`;
}

export function cpu(millis?: number | null): string {
  if (millis === undefined || millis === null || Number.isNaN(millis)) return "—";
  if (millis >= 1000) return `${(millis / 1000).toFixed(millis >= 10000 ? 0 : 2)} cores`;
  if (millis >= 1) return `${Math.round(millis)}m`;
  if (millis === 0) return "0";
  return `${millis.toFixed(2)}m`;
}

export function percent(part?: number | null, total?: number | null): number | null {
  if (part == null || !total) return null;
  return Math.min(100, Math.max(0, (part / total) * 100));
}

export function dateTime(ts?: string | null): string {
  if (!ts) return "—";
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString(undefined, { year: "numeric", month: "short", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function pluralize(n: number, word: string, plural = `${word}s`): string {
  return `${n} ${n === 1 ? word : plural}`;
}

/** Simple case-insensitive multi-term matcher: all whitespace-separated terms must match. */
export function matchesFilter(haystack: string, filter: string): boolean {
  const terms = filter.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return true;
  const h = haystack.toLowerCase();
  return terms.every((t) => h.includes(t));
}

export function cn(...classes: unknown[]): string {
  return classes.filter((c): c is string => typeof c === "string" && c.length > 0).join(" ");
}
