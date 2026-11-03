import { useQuery } from "@tanstack/react-query";
import { Copy, ExternalLink, FileCode2, ScrollText, SquareTerminal, X } from "lucide-react";
import React, { lazy, Suspense, useEffect, useState } from "react";
import { Badge, Button, EmptyState, ErrorBanner, KeyValue, Spinner, StatusText, Tabs, Tooltip } from "@/components/ui";
import { useWatch } from "@/hooks/useWatch";
import { age, bytes, cpu, dateTime } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { Health, Row } from "@/lib/types";
import { useApp, type Selection } from "@/store/app";
import { runAction } from "./actions";
import { kindByRef, KINDS } from "./catalog";
import { ActionMenu } from "./ResourceView";
import { summarizeContainers } from "./podInfo";

const YamlViewer = lazy(() => import("@/features/editor/YamlViewer"));

type Tab = "overview" | "yaml" | "events" | "pods";

function Labels({ labels }: { labels?: Record<string, string> }) {
  const entries = Object.entries(labels ?? {});
  if (!entries.length) return <span className="text-subtle">—</span>;
  return (
    <div className="flex flex-wrap gap-1">
      {entries.map(([k, v]) => (
        <span key={k} className="selectable max-w-full truncate rounded border border-border bg-canvas px-1.5 py-0.5 font-mono text-[11px] text-muted" title={`${k}=${v}`}>
          {k}
          {v ? `=${v}` : ""}
        </span>
      ))}
    </div>
  );
}

function Conditions({ conditions }: { conditions?: any[] }) {
  if (!conditions?.length) return null;
  return (
    <Block title="Conditions">
      <div className="flex flex-col divide-y divide-border rounded-md border border-border">
        {conditions.map((c) => {
          const good = c.status === "True" ? !/Pressure|Unavailable|Failure|Failed/.test(c.type) : /Pressure|Unavailable/.test(c.type);
          return (
            <div key={c.type} className="flex flex-col gap-0.5 px-3 py-2">
              <div className="flex items-center justify-between gap-2">
                <StatusText health={good ? "ok" : c.status === "Unknown" ? "neutral" : "warning"}>{c.type}</StatusText>
                <span className="text-[11.5px] text-subtle">{c.lastTransitionTime ? `${age(c.lastTransitionTime)} ago` : ""}</span>
              </div>
              {(c.reason || c.message) && (
                <div className="selectable text-[12px] text-muted">
                  {c.reason}
                  {c.message ? ` – ${c.message}` : ""}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </Block>
  );
}

function Block({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <h4 className="text-[11px] font-semibold tracking-wide text-subtle uppercase">{title}</h4>
      {children}
    </div>
  );
}

function PodDetails({ obj, sel }: { obj: any; sel: Selection }) {
  const containers = summarizeContainers(obj);
  const openTerminal = useApp((s) => s.openTerminal);
  const openLogs = useApp((s) => s.openLogs);
  return (
    <>
      <KeyValue
        items={[
          ["Node", obj.spec?.nodeName],
          ["Pod IP", obj.status?.podIP],
          ["QoS class", obj.status?.qosClass],
          ["Service account", obj.spec?.serviceAccountName],
          ["Restart policy", obj.spec?.restartPolicy],
        ]}
      />
      <Block title="Containers">
        <div className="flex flex-col gap-2">
          {containers.map((c) => (
            <div key={c.name} className="rounded-md border border-border p-3">
              <div className="flex items-center justify-between gap-2">
                <div className="flex min-w-0 items-center gap-2">
                  <StatusText health={c.health}>{c.name}</StatusText>
                  {c.init && <Badge>init</Badge>}
                </div>
                {!c.init && (
                  <div className="flex gap-1">
                    <Tooltip content="Logs">
                      <Button size="icon-sm" variant="ghost" onClick={() => openLogs({ clusterId: sel.clusterId, namespace: sel.namespace!, pods: [sel.name], container: c.name, title: `${sel.name} / ${c.name}` })}>
                        <ScrollText className="size-3.5" />
                      </Button>
                    </Tooltip>
                    <Tooltip content="Shell">
                      <Button size="icon-sm" variant="ghost" onClick={() => openTerminal({ clusterId: sel.clusterId, namespace: sel.namespace!, pod: sel.name, container: c.name, shell: "auto" })}>
                        <SquareTerminal className="size-3.5" />
                      </Button>
                    </Tooltip>
                  </div>
                )}
              </div>
              <div className="selectable mt-1 truncate font-mono text-[11.5px] text-muted" title={c.image}>
                {c.image}
              </div>
              <div className="mt-2 grid grid-cols-3 gap-2 text-[12px]">
                <div>
                  <div className="text-subtle">State</div>
                  <div>{c.state}</div>
                </div>
                <div>
                  <div className="text-subtle">Restarts</div>
                  <div className={c.restarts ? "text-warn" : ""}>{c.restarts}</div>
                </div>
                <div>
                  <div className="text-subtle">Requests</div>
                  <div className="truncate">{c.requests || "—"}</div>
                </div>
              </div>
              {c.lastTermination && <div className="mt-2 text-[12px] text-warn">Last terminated: {c.lastTermination}</div>}
              {c.ports && <div className="mt-1 text-[12px] text-muted">Ports: {c.ports}</div>}
            </div>
          ))}
        </div>
      </Block>
      <Conditions conditions={obj.status?.conditions} />
    </>
  );
}

function WorkloadDetails({ obj, row }: { obj: any; row?: Row }) {
  const spec = obj.spec ?? {};
  const st = obj.status ?? {};
  return (
    <>
      <KeyValue
        items={[
          ["Replicas", spec.replicas !== undefined ? `${st.readyReplicas ?? st.numberReady ?? 0} ready / ${spec.replicas ?? st.desiredNumberScheduled} desired` : undefined],
          ["Strategy", spec.strategy?.type ?? spec.updateStrategy?.type],
          ["Selector", row?.cells.selector],
          ["Images", (row?.cells.images as string[] | undefined)?.join(", ")],
          ["Paused", spec.paused ? "Yes" : undefined],
          ["Revision", obj.metadata?.annotations?.["deployment.kubernetes.io/revision"]],
        ].filter(([, v]) => v !== undefined) as [string, React.ReactNode][]}
      />
      <Conditions conditions={st.conditions} />
    </>
  );
}

function NodeDetails({ obj }: { obj: any }) {
  const info = obj.status?.nodeInfo ?? {};
  const cap = obj.status?.capacity ?? {};
  const alloc = obj.status?.allocatable ?? {};
  return (
    <>
      <KeyValue
        items={[
          ["Kubelet", info.kubeletVersion],
          ["OS", `${info.osImage ?? ""} (${info.architecture ?? ""})`],
          ["Kernel", info.kernelVersion],
          ["Runtime", info.containerRuntimeVersion],
          ["CPU", `${cap.cpu} capacity · ${alloc.cpu} allocatable`],
          ["Memory", `${cap.memory} capacity · ${alloc.memory} allocatable`],
          ["Pods", `${cap.pods} capacity`],
          ["Addresses", (obj.status?.addresses ?? []).map((a: any) => `${a.type}: ${a.address}`).join(", ")],
        ]}
      />
      {obj.spec?.taints?.length > 0 && (
        <Block title="Taints">
          <div className="flex flex-col gap-1 font-mono text-[12px]">
            {obj.spec.taints.map((t: any, i: number) => (
              <span key={i} className="selectable">
                {t.key}
                {t.value ? `=${t.value}` : ""}:{t.effect}
              </span>
            ))}
          </div>
        </Block>
      )}
      <Conditions conditions={obj.status?.conditions} />
    </>
  );
}

function ServiceDetails({ obj }: { obj: any }) {
  return (
    <>
      <KeyValue
        items={[
          ["Type", obj.spec?.type],
          ["Cluster IP", (obj.spec?.clusterIPs ?? [obj.spec?.clusterIP]).join(", ")],
          ["Session affinity", obj.spec?.sessionAffinity],
          ["Selector", Object.entries(obj.spec?.selector ?? {}).map(([k, v]) => `${k}=${v}`).join(", ") || "—"],
        ]}
      />
      <Block title="Ports">
        <div className="flex flex-col gap-1 font-mono text-[12px]">
          {(obj.spec?.ports ?? []).map((p: any, i: number) => (
            <span key={i} className="selectable">
              {p.name ? `${p.name}: ` : ""}
              {p.port} → {p.targetPort}/{p.protocol}
              {p.nodePort ? ` (node ${p.nodePort})` : ""}
            </span>
          ))}
        </div>
      </Block>
    </>
  );
}

function SecretOrConfigData({ obj, secret }: { obj: any; secret: boolean }) {
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const data: Record<string, string> = obj.data ?? {};
  const entries = Object.entries(data);
  if (!entries.length) return null;
  const decode = (v: string) => {
    try {
      return new TextDecoder().decode(Uint8Array.from(atob(v), (c) => c.charCodeAt(0)));
    } catch {
      return "<binary>";
    }
  };
  return (
    <Block title="Data">
      <div className="flex flex-col gap-2">
        {entries.map(([k, v]) => (
          <div key={k} className="rounded-md border border-border">
            <div className="flex items-center justify-between border-b border-border px-3 py-1.5">
              <span className="font-mono text-[12px] font-medium">{k}</span>
              {secret && (
                <Button size="sm" variant="ghost" onClick={() => setRevealed({ ...revealed, [k]: !revealed[k] })}>
                  {revealed[k] ? "Hide" : "Reveal"}
                </Button>
              )}
            </div>
            <pre className="max-h-48 overflow-auto px-3 py-2 font-mono text-[11.5px] whitespace-pre-wrap text-muted">
              {secret ? (revealed[k] ? decode(v) : "••••••••") : v}
            </pre>
          </div>
        ))}
      </div>
    </Block>
  );
}

function EventsTab({ sel, uid }: { sel: Selection; uid?: string }) {
  const { rows, ready } = useWatch(sel.clusterId, KINDS.events.ref, sel.namespace ?? "", { fieldSelector: uid ? `involvedObject.uid=${uid}` : undefined, enabled: !!uid });
  if (!ready) return <Spinner />;
  if (!rows.length) return <EmptyState title="No events" description="Events are retained by the cluster for about one hour." />;
  const sorted = rows.slice().sort((a, b) => (Date.parse(b.cells.lastSeen) || 0) - (Date.parse(a.cells.lastSeen) || 0));
  return (
    <div className="flex flex-col gap-2">
      {sorted.map((e) => (
        <div key={e.uid} className="rounded-md border border-border p-2.5">
          <div className="flex items-center justify-between gap-2">
            <StatusText health={e.health as Health}>{e.cells.reason}</StatusText>
            <span className="text-[11.5px] text-subtle">
              {e.cells.count > 1 ? `${e.cells.count}× · ` : ""}
              {age(e.cells.lastSeen)} ago
            </span>
          </div>
          <div className="selectable mt-1 text-[12.5px] text-muted">{e.cells.message}</div>
        </div>
      ))}
    </div>
  );
}

function PodsTab({ sel, selector }: { sel: Selection; selector: string }) {
  const select = useApp((s) => s.select);
  const { rows, ready } = useWatch(sel.clusterId, KINDS.pods.ref, sel.namespace ?? "", { labelSelector: selector, enabled: !!selector });
  if (!ready) return <Spinner />;
  if (!rows.length) return <EmptyState title="No pods" />;
  return (
    <div className="flex flex-col divide-y divide-border rounded-md border border-border">
      {rows.map((p) => (
        <button key={p.uid} className="flex items-center justify-between gap-2 px-3 py-2 text-left hover:bg-hover" onClick={() => select({ clusterId: sel.clusterId, resource: KINDS.pods.ref, namespace: p.namespace, name: p.name })}>
          <span className="truncate">{p.name}</span>
          <span className="flex items-center gap-3 text-[12px]">
            <span className="tabular text-muted">{p.cells.ready}</span>
            <StatusText health={p.health}>{p.status}</StatusText>
          </span>
        </button>
      ))}
    </div>
  );
}

export function DetailPanel() {
  const sel = useApp((s) => s.selection);
  const select = useApp((s) => s.select);
  const toast = useApp((s) => s.toast);
  const [tab, setTab] = useState<Tab>("overview");
  useEffect(() => setTab((sel?.tab as Tab) ?? "overview"), [sel?.name, sel?.resource.kind, sel?.tab]);
  const def = sel ? kindByRef(sel.resource) : null;

  // Row for live status (shares the list watch cache in the core).
  const { rows } = useWatch(sel?.clusterId, sel?.resource, sel?.resource.namespaced ? sel?.namespace ?? "" : "", {
    fieldSelector: sel ? `metadata.name=${sel.name}` : undefined,
    enabled: !!sel,
  });
  const row = rows[0];

  const q = useQuery({
    queryKey: ["object", sel?.clusterId, sel?.resource.plural, sel?.namespace, sel?.name, row?.resourceVersion],
    enabled: !!sel,
    queryFn: () => api.getResource(sel!.clusterId, sel!.resource, sel!.namespace, sel!.name),
    placeholderData: (prev) => prev,
  });

  if (!sel || !def) return null;
  const obj = q.data?.object;
  const kind = sel.resource.kind;
  const selector = row?.cells.selector as string | undefined;
  const isWorkload = ["Deployment", "StatefulSet", "DaemonSet", "ReplicaSet", "Job"].includes(kind);

  return (
    <aside className="flex w-[440px] shrink-0 animate-slide-in flex-col border-l border-border bg-panel">
      <div className="flex shrink-0 flex-col gap-2 border-b border-border px-4 pt-3 pb-2">
        <div className="flex items-start justify-between gap-2">
          <div className="flex min-w-0 flex-col">
            <span className="text-[11.5px] font-medium text-subtle">{kind}</span>
            <span className="selectable truncate text-[15px] font-semibold" title={sel.name}>
              {sel.name}
            </span>
            <div className="mt-1 flex items-center gap-2 text-[12px] text-muted">
              {row && <StatusText health={row.health}>{row.status || "—"}</StatusText>}
              {sel.namespace && <Badge>{sel.namespace}</Badge>}
            </div>
          </div>
          <div className="flex items-center gap-0.5">
            <Tooltip content="Copy name">
              <Button
                size="icon-sm"
                variant="ghost"
                onClick={() => navigator.clipboard.writeText(sel.name).then(() => toast({ kind: "info", title: "Copied to clipboard" }))}
              >
                <Copy className="size-3.5" />
              </Button>
            </Tooltip>
            {row && <ActionMenu clusterId={sel.clusterId} def={def} row={row} />}
            <Button size="icon-sm" variant="ghost" onClick={() => select(null)} aria-label="Close details">
              <X className="size-4" />
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap gap-1.5">
          {def.actions.includes("logs") && row && (
            <Button size="sm" icon={<ScrollText className="size-3.5" />} onClick={() => runAction("logs", { clusterId: sel.clusterId, ref: def.ref, namespace: sel.namespace, name: sel.name, row })}>
              Logs
            </Button>
          )}
          {def.actions.includes("shell") && row && (
            <Button size="sm" icon={<SquareTerminal className="size-3.5" />} onClick={() => runAction("shell", { clusterId: sel.clusterId, ref: def.ref, namespace: sel.namespace, name: sel.name, row })}>
              Shell
            </Button>
          )}
          <Button size="sm" icon={<FileCode2 className="size-3.5" />} onClick={() => runAction("edit", { clusterId: sel.clusterId, ref: def.ref, namespace: sel.namespace, name: sel.name, row })}>
            Edit
          </Button>
          <Tooltip content="Open in new window">
            <Button size="sm" variant="ghost" icon={<ExternalLink className="size-3.5" />} onClick={() => api.openWindow(sel.clusterId).catch(() => {})} />
          </Tooltip>
        </div>
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { id: "overview", label: "Overview" },
            { id: "pods", label: "Pods", hidden: !isWorkload || !selector },
            { id: "events", label: "Events", hidden: kind === "Event" },
            { id: "yaml", label: "YAML" },
          ]}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-4 py-4">
        {q.error && <ErrorBanner message={errorMessage(q.error)} />}
        {!obj && !q.error && <Spinner />}
        {obj && tab === "overview" && (
          <div className="flex flex-col gap-5">
            <KeyValue
              items={[
                ["Created", obj.metadata?.creationTimestamp ? `${dateTime(obj.metadata.creationTimestamp)} (${age(obj.metadata.creationTimestamp)})` : undefined],
                ["UID", <span className="font-mono text-[11.5px]">{obj.metadata?.uid}</span>],
                ...(row?.cells.ownerKind ? [["Controlled by", `${row.cells.ownerKind}/${row.cells.ownerName}`] as [string, string]] : []),
              ]}
            />
            {kind === "Pod" && <PodDetails obj={obj} sel={sel} />}
            {isWorkload && <WorkloadDetails obj={obj} row={row} />}
            {kind === "Node" && <NodeDetails obj={obj} />}
            {kind === "Service" && <ServiceDetails obj={obj} />}
            {(kind === "ConfigMap" || kind === "Secret") && <SecretOrConfigData obj={obj} secret={kind === "Secret"} />}
            {kind === "Node" && row && (
              <KeyValue
                items={[
                  ["CPU capacity", cpu(row.cells.cpuCapacityMillis)],
                  ["Memory capacity", bytes(row.cells.memoryCapacityBytes)],
                ]}
              />
            )}
            {!["Pod", "Node", "Service"].includes(kind) && !isWorkload && <Conditions conditions={obj.status?.conditions} />}
            <Block title="Labels">
              <Labels labels={obj.metadata?.labels} />
            </Block>
            <Block title="Annotations">
              <Labels labels={Object.fromEntries(Object.entries(obj.metadata?.annotations ?? {}).filter(([k]) => k !== "kubectl.kubernetes.io/last-applied-configuration")) as Record<string, string>} />
            </Block>
          </div>
        )}
        {obj && tab === "events" && <EventsTab sel={sel} uid={obj.metadata?.uid} />}
        {obj && tab === "pods" && selector && <PodsTab sel={sel} selector={selector} />}
        {q.data && tab === "yaml" && (
          <div className="-mx-4 -my-4 h-[calc(100%+2rem)]">
            <Suspense fallback={<Spinner />}>
              <YamlViewer value={q.data.yaml} />
            </Suspense>
          </div>
        )}
      </div>
    </aside>
  );
}
