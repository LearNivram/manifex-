import { describe, expect, it } from "vitest";
import { age, bytes, cpu, matchesFilter, percent } from "./format";

describe("format", () => {
  const now = Date.parse("2024-01-10T12:00:00Z");

  it("formats ages like kubectl", () => {
    expect(age("2024-01-10T11:59:30Z", now)).toBe("30s");
    expect(age("2024-01-10T11:15:00Z", now)).toBe("45m");
    expect(age("2024-01-10T09:30:00Z", now)).toBe("2h30m");
    expect(age("2024-01-08T12:00:00Z", now)).toBe("2d");
    expect(age(null, now)).toBe("—");
  });

  it("formats bytes with binary units", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(1536)).toBe("1.5 Ki");
    expect(bytes(8 * 1024 ** 3)).toBe("8.0 Gi");
  });

  it("formats cpu", () => {
    expect(cpu(250)).toBe("250m");
    expect(cpu(2000)).toBe("2.00 cores");
    expect(cpu(0)).toBe("0");
  });

  it("computes percentages safely", () => {
    expect(percent(1, 4)).toBe(25);
    expect(percent(5, 0)).toBeNull();
    expect(percent(10, 5)).toBe(100);
  });

  it("matches multi-term filters", () => {
    expect(matchesFilter("nginx-abc default Running", "nginx run")).toBe(true);
    expect(matchesFilter("nginx-abc default Running", "redis")).toBe(false);
    expect(matchesFilter("x", "  ")).toBe(true);
  });
});
