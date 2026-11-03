import type { Health } from "@/lib/types";

export interface ContainerInfo {
  name: string;
  image: string;
  init: boolean;
  state: string;
  health: Health;
  restarts: number;
  requests: string;
  ports: string;
  lastTermination?: string;
}

/** Derives a per-container view from a Pod object (spec + status). */
export function summarizeContainers(pod: any): ContainerInfo[] {
  const statuses = new Map<string, any>();
  for (const s of [...(pod?.status?.initContainerStatuses ?? []), ...(pod?.status?.containerStatuses ?? [])]) statuses.set(s.name, s);
  const build = (c: any, init: boolean): ContainerInfo => {
    const s = statuses.get(c.name);
    let state = "Waiting";
    let health: Health = "pending";
    if (s?.state?.running) {
      state = s.ready ? "Running" : "Running (not ready)";
      health = s.ready ? "ok" : "warning";
    } else if (s?.state?.terminated) {
      const t = s.state.terminated;
      state = `${t.reason ?? "Terminated"} (exit ${t.exitCode})`;
      health = t.exitCode === 0 ? "neutral" : "error";
    } else if (s?.state?.waiting) {
      state = s.state.waiting.reason ?? "Waiting";
      health = /BackOff|Err|Invalid/.test(state) ? "error" : "pending";
    }
    const req = c.resources?.requests ?? {};
    const lt = s?.lastState?.terminated;
    return {
      name: c.name,
      image: c.image ?? "",
      init,
      state,
      health,
      restarts: s?.restartCount ?? 0,
      requests: [req.cpu && `cpu ${req.cpu}`, req.memory && `mem ${req.memory}`].filter(Boolean).join(", "),
      ports: (c.ports ?? []).map((p: any) => `${p.containerPort}/${p.protocol ?? "TCP"}${p.name ? ` (${p.name})` : ""}`).join(", "),
      lastTermination: lt ? `${lt.reason ?? "Error"} (exit ${lt.exitCode})${lt.finishedAt ? ` at ${new Date(lt.finishedAt).toLocaleString()}` : ""}` : undefined,
    };
  };
  return [...(pod?.spec?.initContainers ?? []).map((c: any) => build(c, true)), ...(pod?.spec?.containers ?? []).map((c: any) => build(c, false))];
}
