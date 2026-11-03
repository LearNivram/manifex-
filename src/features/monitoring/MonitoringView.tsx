import { useQuery } from "@tanstack/react-query";
import { Bell, Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Badge, Button, Dialog, EmptyState, ErrorBanner, Field, Input, Section, Select, Spinner, StatusDot, Tabs, Toggle } from "@/components/ui";
import { LineChart, type Series } from "@/components/ui/charts";
import { age, bytes, cpu } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { AlertRule, PodMetric } from "@/lib/types";
import { KINDS } from "@/features/resources/catalog";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useActiveCluster, useApp, useNamespace } from "@/store/app";

const PALETTE = ["#6d7bff", "#3ccf7a", "#e7a93b", "#f26565", "#38bdf8", "#c084fc", "#f472b6", "#94a3b8"];
const MAX_POINTS = 120;

/** Samples metrics while the view is open (in-memory ring buffer, nothing persisted). */
function useMetricHistory(clusterId: string) {
  const [history, setHistory] = useState<{ t: number; cpu: number; mem: number; nodes: Record<string, { cpu: number; mem: number }> }[]>([]);
  const q = useQuery({ queryKey: ["node-metrics", clusterId], queryFn: () => api.nodeMetrics(clusterId), refetchInterval: 15_000, retry: false });
  const last = useRef(0);
  useEffect(() => {
    if (!q.data || q.dataUpdatedAt === last.current) return;
    last.current = q.dataUpdatedAt;
    const nodes: Record<string, { cpu: number; mem: number }> = {};
    for (const n of q.data) nodes[n.name] = { cpu: n.cpuMillis, mem: n.memoryBytes };
    setHistory((h) =>
      [...h, { t: q.dataUpdatedAt, cpu: q.data.reduce((a, n) => a + n.cpuMillis, 0), mem: q.data.reduce((a, n) => a + n.memoryBytes, 0), nodes }].slice(-MAX_POINTS),
    );
  }, [q.data, q.dataUpdatedAt]);
  useEffect(() => setHistory([]), [clusterId]);
  return { history, error: q.error, loading: q.isLoading };
}

function TopPods({ metrics, by }: { metrics: PodMetric[]; by: "cpu" | "mem" }) {
  const select = useApp((s) => s.select);
  const clusterId = useApp((s) => s.activeClusterId)!;
  const sorted = useMemo(() => metrics.slice().sort((a, b) => (by === "cpu" ? b.cpuMillis - a.cpuMillis : b.memoryBytes - a.memoryBytes)).slice(0, 15), [metrics, by]);
  const max = by === "cpu" ? sorted[0]?.cpuMillis ?? 1 : sorted[0]?.memoryBytes ?? 1;
  return (
    <div className="divide-y divide-border">
      {sorted.map((p) => {
        const v = by === "cpu" ? p.cpuMillis : p.memoryBytes;
        return (
          <button
            key={`${p.namespace}/${p.name}`}
            onClick={() => select({ clusterId, resource: KINDS.pods.ref, namespace: p.namespace, name: p.name })}
            className="grid w-full grid-cols-[1fr_140px_80px] items-center gap-3 px-4 py-2 text-left text-[12.5px] hover:bg-hover"
          >
            <span className="flex min-w-0 flex-col">
              <span className="truncate font-medium">{p.name}</span>
              <span className="truncate text-[11.5px] text-muted">{p.namespace}</span>
            </span>
            <div className="h-1.5 overflow-hidden rounded-full bg-hover">
              <div className="h-full rounded-full bg-accent" style={{ width: `${(v / max) * 100}%` }} />
            </div>
            <span className="tabular text-right">{by === "cpu" ? cpu(v) : bytes(v)}</span>
          </button>
        );
      })}
    </div>
  );
}

function RuleDialog({ rule, onClose }: { rule: AlertRule | null; onClose: () => void }) {
  const clusters = useApp((s) => s.clusters);
  const [form, setForm] = useState<AlertRule>(
    rule ?? { id: "", name: "", enabled: true, clusterId: null, namespace: "", kind: "", reasonPattern: "", messagePattern: "", severity: "warning", createdAt: "" },
  );
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    try {
      await api.saveAlertRule(form);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={rule ? "Edit alert rule" : "New alert rule"}
      description="Rules match Kubernetes Warning events. Patterns are regular expressions."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!form.name.trim()} onClick={save}>
            Save rule
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-4">
        <Field label="Name">
          <Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="OOM in production" autoFocus />
        </Field>
        <Field label="Severity">
          <Select value={form.severity} onChange={(v) => setForm({ ...form, severity: v as AlertRule["severity"] })} options={["info", "warning", "critical"].map((s) => ({ value: s, label: s }))} />
        </Field>
        <Field label="Cluster">
          <Select
            value={form.clusterId ?? ""}
            onChange={(v) => setForm({ ...form, clusterId: v || null })}
            options={[{ value: "", label: "All clusters" }, ...clusters.map((c) => ({ value: c.profile.id, label: c.profile.name }))]}
          />
        </Field>
        <Field label="Namespace">
          <Input value={form.namespace ?? ""} onChange={(e) => setForm({ ...form, namespace: e.target.value })} placeholder="any" />
        </Field>
        <Field label="Object kind">
          <Input value={form.kind ?? ""} onChange={(e) => setForm({ ...form, kind: e.target.value })} placeholder="Pod, Deployment, …" />
        </Field>
        <Field label="Reason pattern">
          <Input value={form.reasonPattern ?? ""} onChange={(e) => setForm({ ...form, reasonPattern: e.target.value })} placeholder="^(OOMKilling|BackOff)$" />
        </Field>
        <div className="col-span-2">
          <Field label="Message pattern">
            <Input value={form.messagePattern ?? ""} onChange={(e) => setForm({ ...form, messagePattern: e.target.value })} placeholder="(?i)timeout" />
          </Field>
        </div>
        <Toggle checked={form.enabled} onChange={(v) => setForm({ ...form, enabled: v })} label="Enabled" />
      </div>
      {error && <ErrorBanner className="mt-4" message={error} />}
    </Dialog>
  );
}

function AlertsPanel() {
  const rules = useQuery({ queryKey: ["alert-rules"], queryFn: api.listAlertRules });
  const allNotices = useApp((s) => s.notices);
  const notices = useMemo(() => allNotices.filter((n) => n.alert), [allNotices]);
  const [editing, setEditing] = useState<AlertRule | null | undefined>(undefined);
  return (
    <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1fr_420px]">
      <Section title="Active alerts" actions={<Badge>{notices.length}</Badge>}>
        {notices.length === 0 ? (
          <EmptyState icon={<Bell className="size-5" />} title="No alerts" description="Alerts are raised for pod failures, workload failures, node problems and your custom rules." />
        ) : (
          <div className="max-h-[60vh] divide-y divide-border overflow-auto">
            {notices.map((n) => (
              <div key={n.id} className="flex items-start gap-3 px-4 py-2.5">
                <StatusDot health={n.severity === "critical" ? "error" : n.severity === "warning" ? "warning" : "pending"} className="mt-1.5" />
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="text-[13px] font-medium">{n.title}</span>
                  {n.message && <span className="selectable text-[12px] text-muted">{n.message}</span>}
                  <span className="text-[11px] text-subtle">
                    {n.clusterName} · {n.alert?.category} · {age(n.ts)} ago
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </Section>
      <Section
        title="Custom alert rules"
        actions={
          <Button size="sm" icon={<Plus className="size-3.5" />} onClick={() => setEditing(null)}>
            New rule
          </Button>
        }
      >
        {rules.data?.length === 0 && <EmptyState title="No custom rules" description="Create rules to get notified about specific events." />}
        <div className="divide-y divide-border">
          {rules.data?.map((r) => (
            <div key={r.id} className="flex items-center gap-3 px-4 py-2.5">
              <button className="flex min-w-0 flex-1 flex-col text-left" onClick={() => setEditing(r)}>
                <span className="flex items-center gap-2 text-[13px] font-medium">
                  {r.name}
                  <Badge tone={r.severity === "critical" ? "error" : r.severity === "warning" ? "warning" : "pending"}>{r.severity}</Badge>
                  {!r.enabled && <Badge>disabled</Badge>}
                </span>
                <span className="truncate font-mono text-[11.5px] text-muted">
                  {[r.kind, r.namespace, r.reasonPattern && `reason~${r.reasonPattern}`, r.messagePattern && `msg~${r.messagePattern}`].filter(Boolean).join(" · ") || "all warning events"}
                </span>
              </button>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label="Delete rule"
                onClick={async () => {
                  await api.deleteAlertRule(r.id);
                  rules.refetch();
                }}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </div>
          ))}
        </div>
      </Section>
      {editing !== undefined && (
        <RuleDialog
          rule={editing}
          onClose={() => {
            setEditing(undefined);
            rules.refetch();
          }}
        />
      )}
    </div>
  );
}

export function MonitoringView() {
  const active = useActiveCluster()!;
  const id = active.profile.id;
  const ns = useNamespace();
  const [tab, setTab] = useState<"metrics" | "alerts">("metrics");
  const { history, error, loading } = useMetricHistory(id);
  const pods = useQuery({ queryKey: ["pod-metrics", id, ns], queryFn: () => api.podMetrics(id, ns || null), refetchInterval: 15_000, retry: false, enabled: tab === "metrics" });
  const capacity = useQuery({ queryKey: ["capacity", id], queryFn: () => api.clusterCapacity(id), refetchInterval: 60_000 });

  const nodeNames = Object.keys(history.at(-1)?.nodes ?? {}).slice(0, PALETTE.length);
  const cpuSeries: Series[] = nodeNames.map((n, i) => ({ name: n, color: PALETTE[i], points: history.filter((h) => h.nodes[n]).map((h) => ({ t: h.t, v: h.nodes[n].cpu })) }));
  const memSeries: Series[] = nodeNames.map((n, i) => ({ name: n, color: PALETTE[i], points: history.filter((h) => h.nodes[n]).map((h) => ({ t: h.t, v: h.nodes[n].mem })) }));
  const totals: Series[] = [
    { name: "CPU used", color: PALETTE[0], points: history.map((h) => ({ t: h.t, v: capacity.data ? (h.cpu / capacity.data.cpu.allocatable) * 100 : 0 })) },
    { name: "Memory used", color: PALETTE[1], points: history.map((h) => ({ t: h.t, v: capacity.data ? (h.mem / capacity.data.memory.allocatable) * 100 : 0 })) },
  ];

  return (
    <div className="flex h-full flex-col">
      <ViewHeader title="Monitoring" description="Live resource usage from metrics.k8s.io, sampled every 15 seconds while this view is open.">
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { id: "metrics", label: "Metrics" },
            { id: "alerts", label: "Alerts & rules" },
          ]}
        />
      </ViewHeader>
      <div className="min-h-0 flex-1 overflow-auto p-5">
        {tab === "alerts" ? (
          <AlertsPanel />
        ) : error ? (
          <EmptyState title="Metrics unavailable" description={`${errorMessage(error)}. Install metrics-server to enable live usage metrics.`} />
        ) : loading ? (
          <Spinner />
        ) : (
          <div className="mx-auto flex max-w-[1400px] flex-col gap-4">
            <Section title="Cluster utilisation (% of allocatable)">
              <div className="p-4">
                <LineChart series={totals} max={100} format={(v) => `${v.toFixed(0)}%`} />
              </div>
            </Section>
            <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
              <Section title="CPU by node">
                <div className="p-4">
                  <LineChart series={cpuSeries} format={(v) => cpu(v)} />
                </div>
              </Section>
              <Section title="Memory by node">
                <div className="p-4">
                  <LineChart series={memSeries} format={(v) => bytes(v, 0)} />
                </div>
              </Section>
            </div>
            <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
              <Section title={`Top pods by CPU${ns ? ` in ${ns}` : ""}`}>{pods.data ? <TopPods metrics={pods.data} by="cpu" /> : <Spinner className="m-4" />}</Section>
              <Section title={`Top pods by memory${ns ? ` in ${ns}` : ""}`}>{pods.data ? <TopPods metrics={pods.data} by="mem" /> : <Spinner className="m-4" />}</Section>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
