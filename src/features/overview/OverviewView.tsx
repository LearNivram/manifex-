import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, ArrowRight, Container, Layers, Server } from "lucide-react";
import React, { useMemo } from "react";
import { Badge, EmptyState, Section, Spinner, StatusDot, StatusText } from "@/components/ui";
import { CapacityBar, Donut } from "@/components/ui/charts";
import { useWatch } from "@/hooks/useWatch";
import { age, bytes, cpu, percent } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { Health, Row } from "@/lib/types";
import { KINDS } from "@/features/resources/catalog";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useActiveCluster, useApp, useNamespace } from "@/store/app";

const H_COLORS: Record<Health, string> = { ok: "var(--ok)", warning: "var(--warn)", error: "var(--err)", pending: "var(--pending)", neutral: "var(--neutral)" };

function countBy(rows: Row[]) {
  const c: Record<Health, number> = { ok: 0, warning: 0, error: 0, pending: 0, neutral: 0 };
  for (const r of rows) c[r.health]++;
  return c;
}

function HealthTile({ title, icon, rows, ready, forbidden, onClick }: { title: string; icon: React.ReactNode; rows: Row[]; ready: boolean; forbidden?: boolean; onClick: () => void }) {
  const c = countBy(rows);
  const problems = c.error + c.warning;
  return (
    <button onClick={onClick} className="flex items-center gap-4 rounded-lg border border-border bg-panel p-4 text-left transition-colors hover:border-border-strong">
      <Donut
        size={64}
        segments={(Object.keys(c) as Health[]).map((h) => ({ value: c[h], color: H_COLORS[h] }))}
        label={forbidden ? <span className="text-subtle">–</span> : ready ? <span className="tabular">{rows.length}</span> : <Spinner />}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="flex items-center gap-2 text-[13px] font-semibold">
          {icon}
          {title}
        </span>
        {forbidden ? (
          <span className="text-[12px] text-muted">No permission to list {title.toLowerCase()} with this identity</span>
        ) : (
          <span className="text-[12px] text-muted">
            {c.ok} healthy
            {c.pending ? ` · ${c.pending} pending` : ""}
            {c.neutral ? ` · ${c.neutral} completed` : ""}
          </span>
        )}
        {forbidden ? null : problems > 0 ? <span className="text-[12px] font-medium text-err">{problems} need attention</span> : <span className="text-[12px] text-ok">All good</span>}
      </div>
      <ArrowRight className="size-4 text-subtle" />
    </button>
  );
}

function ResourceMeter({ label, used, requests, limits, allocatable, fmt }: { label: string; used?: number | null; requests: number; limits: number; allocatable: number; fmt: (n: number) => string }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between text-[12.5px]">
        <span className="font-medium">{label}</span>
        <span className="tabular text-muted">
          {used != null ? `${fmt(used)} used · ` : ""}
          {fmt(requests)} requested of {fmt(allocatable)}
        </span>
      </div>
      <CapacityBar usage={used} requests={requests} limits={limits} allocatable={allocatable} />
      <div className="flex gap-4 text-[11.5px] text-subtle">
        {used != null && <span>usage {percent(used, allocatable)?.toFixed(0)}%</span>}
        <span>requests {percent(requests, allocatable)?.toFixed(0)}%</span>
        <span>limits {percent(limits, allocatable)?.toFixed(0)}%</span>
      </div>
    </div>
  );
}

export function OverviewView() {
  const active = useActiveCluster()!;
  const id = active.profile.id;
  const ns = useNamespace();
  const navigate = useApp((s) => s.navigate);
  const select = useApp((s) => s.select);
  const pods = useWatch(id, KINDS.pods.ref, ns);
  const deployments = useWatch(id, KINDS.deployments.ref, ns);
  const nodes = useWatch(id, KINDS.nodes.ref, null);
  const warnings = useWatch(id, KINDS.events.ref, ns, { fieldSelector: "type=Warning" });
  const capacity = useQuery({ queryKey: ["capacity", id], queryFn: () => api.clusterCapacity(id), refetchInterval: 30_000 });

  const problemPods = useMemo(() => pods.rows.filter((p) => p.health === "error" || p.health === "warning").slice(0, 12), [pods.rows]);
  const recentWarnings = useMemo(
    () => warnings.rows.slice().sort((a, b) => (Date.parse(b.cells.lastSeen) || 0) - (Date.parse(a.cells.lastSeen) || 0)).slice(0, 10),
    [warnings.rows],
  );
  const cap = capacity.data;

  return (
    <div className="flex h-full flex-col">
      <ViewHeader
        title="Overview"
        description={`${active.status?.version ?? ""} · ${active.status?.server ?? ""}${ns ? ` · namespace ${ns}` : ""}`}
      />
      <div className="min-h-0 flex-1 overflow-auto p-5">
        <div className="mx-auto flex max-w-[1400px] flex-col gap-4">
          <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
            <HealthTile title="Nodes" icon={<Server className="size-4 text-muted" />} rows={nodes.rows} ready={nodes.ready} forbidden={nodes.forbidden} onClick={() => navigate("nodes")} />
            <HealthTile title="Deployments" icon={<Layers className="size-4 text-muted" />} rows={deployments.rows} ready={deployments.ready} forbidden={deployments.forbidden} onClick={() => navigate("deployments")} />
            <HealthTile title="Pods" icon={<Container className="size-4 text-muted" />} rows={pods.rows} ready={pods.ready} forbidden={pods.forbidden} onClick={() => navigate("pods")} />
          </div>

          <Section
            title="Capacity"
            actions={cap && !cap.metricsAvailable ? <Badge tone="warning">metrics-server not installed – usage unavailable</Badge> : null}
          >
            <div className="grid grid-cols-1 gap-6 p-4 lg:grid-cols-3">
              {capacity.isLoading && <Spinner />}
              {cap && (
                <>
                  <ResourceMeter label="CPU" used={cap.cpu.usage} requests={cap.cpu.requests} limits={cap.cpu.limits} allocatable={cap.cpu.allocatable} fmt={cpu} />
                  <ResourceMeter label="Memory" used={cap.memory.usage} requests={cap.memory.requests} limits={cap.memory.limits} allocatable={cap.memory.allocatable} fmt={(n) => bytes(n, 1)} />
                  <ResourceMeter label="Pods" requests={cap.pods} limits={0} allocatable={cap.podCapacity} fmt={(n) => String(Math.round(n))} />
                </>
              )}
            </div>
          </Section>

          <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <Section title="Pods needing attention" actions={<Badge tone={problemPods.length ? "error" : "ok"}>{problemPods.length}</Badge>}>
              {problemPods.length === 0 ? (
                <EmptyState title="No failing pods" description="Pods in error or degraded states appear here." />
              ) : (
                <div className="divide-y divide-border">
                  {problemPods.map((p) => (
                    <button
                      key={p.uid}
                      onClick={() => select({ clusterId: id, resource: KINDS.pods.ref, namespace: p.namespace, name: p.name })}
                      className="flex w-full items-center gap-3 px-4 py-2 text-left hover:bg-hover"
                    >
                      <div className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-[13px] font-medium">{p.name}</span>
                        <span className="text-[12px] text-muted">
                          {p.namespace} · {p.cells.restarts} restarts
                        </span>
                      </div>
                      <StatusText health={p.health}>{p.status}</StatusText>
                    </button>
                  ))}
                </div>
              )}
            </Section>
            <Section
              title="Recent warnings"
              actions={
                <button className="text-[12px] text-accent hover:underline" onClick={() => navigate("events")}>
                  All events
                </button>
              }
            >
              {recentWarnings.length === 0 ? (
                <EmptyState title="No warnings" description="Warning events from the last hour appear here." />
              ) : (
                <div className="divide-y divide-border">
                  {recentWarnings.map((e) => (
                    <div key={e.uid} className="flex items-start gap-3 px-4 py-2">
                      <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-warn" />
                      <div className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-[12.5px]">
                          <span className="font-medium">{e.cells.reason}</span> · {e.cells.objectKind}/{e.cells.objectName}
                        </span>
                        <span className="selectable line-clamp-2 text-[12px] text-muted">{e.cells.message}</span>
                      </div>
                      <span className="shrink-0 text-[11.5px] text-subtle">{age(e.cells.lastSeen)}</span>
                    </div>
                  ))}
                </div>
              )}
            </Section>
          </div>

          {cap && cap.nodes.length > 0 && (
            <Section title="Nodes">
              <div className="divide-y divide-border">
                {cap.nodes.map((n) => (
                  <div key={n.name} className="grid grid-cols-[minmax(160px,1.2fr)_1fr_1fr_120px] items-center gap-6 px-4 py-2.5 text-[12.5px]">
                    <span className="flex items-center gap-2 truncate font-medium">
                      <StatusDot health={n.ready ? "ok" : "error"} />
                      {n.name}
                    </span>
                    <div className="flex flex-col gap-1">
                      <CapacityBar usage={n.cpu.usage} requests={n.cpu.requests} limits={n.cpu.limits} allocatable={n.cpu.allocatable} />
                      <span className="tabular text-[11px] text-subtle">
                        CPU {n.cpu.usage != null ? cpu(n.cpu.usage) : "–"} / {cpu(n.cpu.allocatable)}
                      </span>
                    </div>
                    <div className="flex flex-col gap-1">
                      <CapacityBar usage={n.memory.usage} requests={n.memory.requests} limits={n.memory.limits} allocatable={n.memory.allocatable} />
                      <span className="tabular text-[11px] text-subtle">
                        Mem {n.memory.usage != null ? bytes(n.memory.usage) : "–"} / {bytes(n.memory.allocatable)}
                      </span>
                    </div>
                    <span className="tabular text-right text-muted">
                      {n.pods}/{n.podCapacity} pods
                    </span>
                  </div>
                ))}
              </div>
            </Section>
          )}
        </div>
      </div>
    </div>
  );
}
