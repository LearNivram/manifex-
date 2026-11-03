import { useQuery } from "@tanstack/react-query";
import { AppWindow, ChevronRight, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge, EmptyState, ErrorBanner, Input, Spinner, StatusDot, StatusText } from "@/components/ui";
import { cn, matchesFilter } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import { KINDS } from "@/features/resources/catalog";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useActiveCluster, useApp, useNamespace } from "@/store/app";

const KIND_KEY: Record<string, string> = {
  Deployment: "deployments",
  StatefulSet: "statefulsets",
  DaemonSet: "daemonsets",
  CronJob: "cronjobs",
  Service: "services",
  Ingress: "ingresses",
};

export function ApplicationsView() {
  const active = useActiveCluster()!;
  const ns = useNamespace();
  const select = useApp((s) => s.select);
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<Set<string>>(new Set());
  const q = useQuery({ queryKey: ["applications", active.profile.id, ns], queryFn: () => api.listApplications(active.profile.id, ns || null), refetchInterval: 20_000 });
  const apps = useMemo(() => (q.data ?? []).filter((a) => matchesFilter(`${a.name} ${a.namespace} ${a.partOf ?? ""} ${a.managedBy ?? ""}`, filter)), [q.data, filter]);
  const byPartOf = useMemo(() => {
    const m = new Map<string, typeof apps>();
    for (const a of apps) m.set(a.partOf ?? "", [...(m.get(a.partOf ?? "") ?? []), a]);
    return Array.from(m.entries()).sort(([a], [b]) => (a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)));
  }, [apps]);

  return (
    <div className="flex h-full flex-col">
      <ViewHeader
        title="Applications"
        description="Workloads grouped by app.kubernetes.io labels, with the services and ingresses that expose them."
        actions={<Input icon={<Search className="size-3.5" />} placeholder="Filter applications…" value={filter} onChange={(e) => setFilter(e.target.value)} className="w-64" />}
      />
      <div className="min-h-0 flex-1 overflow-auto p-5">
        {q.isLoading && <Spinner />}
        {q.error && <ErrorBanner message={errorMessage(q.error)} />}
        {q.data && apps.length === 0 && <EmptyState icon={<AppWindow className="size-5" />} title="No applications" description="Label workloads with app.kubernetes.io/name or app.kubernetes.io/instance to group them." />}
        <div className="mx-auto flex max-w-[1400px] flex-col gap-6">
          {byPartOf.map(([partOf, list]) => (
            <section key={partOf || "_"} className="flex flex-col gap-2">
              {partOf && <h2 className="text-[12px] font-semibold tracking-wide text-subtle uppercase">Part of {partOf}</h2>}
              <div className="overflow-hidden rounded-lg border border-border bg-panel">
                {list.map((a) => {
                  const key = `${a.namespace}/${a.name}`;
                  const expanded = open.has(key);
                  return (
                    <div key={key} className="border-b border-border last:border-0">
                      <button
                        className="flex w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-hover"
                        onClick={() => setOpen((s) => (s.has(key) ? new Set([...s].filter((x) => x !== key)) : new Set([...s, key])))}
                      >
                        <ChevronRight className={cn("size-4 text-subtle transition-transform", expanded && "rotate-90")} />
                        <StatusDot health={a.health} />
                        <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{a.name}</span>
                        {a.version && <Badge>v{a.version}</Badge>}
                        {a.managedBy && <Badge tone="accent">{a.managedBy}</Badge>}
                        <span className="w-40 truncate text-right text-[12px] text-muted">{a.namespace}</span>
                        <span className="w-28 text-right text-[12px] text-subtle">{a.components.length} components</span>
                      </button>
                      {expanded && (
                        <div className="border-t border-border bg-canvas py-1">
                          {a.components.map((c) => (
                            <button
                              key={`${c.kind}/${c.name}`}
                              className="flex w-full items-center gap-3 py-1.5 pr-4 pl-14 text-left text-[12.5px] hover:bg-hover"
                              onClick={() => {
                                const def = KINDS[KIND_KEY[c.kind]];
                                if (def) select({ clusterId: active.profile.id, resource: def.ref, namespace: c.namespace, name: c.name });
                              }}
                            >
                              <span className="w-24 text-muted">{c.kind}</span>
                              <span className="min-w-0 flex-1 truncate">{c.name}</span>
                              <StatusText health={c.health}>{c.status || "—"}</StatusText>
                            </button>
                          ))}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
