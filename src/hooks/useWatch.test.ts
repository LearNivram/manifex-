import { describe, expect, it } from "vitest";
import { applyWatchMessage } from "./useWatch";
import type { Row } from "@/lib/types";

const row = (uid: string, status = "Running"): Row => ({ uid, name: uid, labels: {}, health: "ok", status, cells: {} });

describe("applyWatchMessage", () => {
  it("replaces state on snapshot and merges batches", () => {
    const m = new Map<string, Row>();
    applyWatchMessage(m, { type: "snapshot", rows: [row("a"), row("b")], ready: true });
    expect(m.size).toBe(2);
    applyWatchMessage(m, { type: "batch", upserts: [row("a", "Terminating"), row("c")], deletes: ["b"] });
    expect([...m.keys()].sort()).toEqual(["a", "c"]);
    expect(m.get("a")!.status).toBe("Terminating");
    applyWatchMessage(m, { type: "snapshot", rows: [], ready: true });
    expect(m.size).toBe(0);
  });

  it("ignores errors", () => {
    const m = new Map<string, Row>([["a", row("a")]]);
    expect(applyWatchMessage(m, { type: "error", message: "x" })).toBe(false);
    expect(m.size).toBe(1);
  });
});
