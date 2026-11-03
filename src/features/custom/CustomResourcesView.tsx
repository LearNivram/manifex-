import { useQueryClient } from "@tanstack/react-query";
import { Puzzle, RefreshCw, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { Button, EmptyState, Input, Spinner, Toggle } from "@/components/ui";
import { ResourceTable } from "@/components/table/ResourceTable";
import { useWatch } from "@/hooks/useWatch";
import { cn, matchesFilter } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { ApiResourceInfo } from "@/lib/types";
import { genericKind } from "@/features/resources/catalog";
import { ActionMenu, useApiResources, ViewHeader } from "@/features/resources/ResourceView";
import { useActiveCluster, useApp, useNamespace } from "@/store/app";

/** Browser for every API resource the cluster serves (including CRDs). */
export function CustomResourcesView() {
  const active = useActiveCluster()!;
  const id = active.profile.id;
  const ns = useNamespace();
  const qc = useQueryClient();
  const select = useApp((s) => s.select);
  const apis = useApiResources(id);
  const [onlyCustom, setOnlyCustom] = useState(true);
  const [filter, setFilter] = useState("");
  const [picked, setPicked] = useState<ApiResourceInfo | null>(null);
  const list = useMemo(
    () => (apis.data ?? []).filter((a) => a.verbs.includes("list") && a.verbs.includes("watch") && (!onlyCustom || a.custom) && matchesFilter(`${a.kind} ${a.group} ${a.plural}`, filter)),
    [apis.data, onlyCustom, filter],
  );
  const groups = useMemo(() => {
    const m = new Map<string, ApiResourceInfo[]>();
    for (const a of list) m.set(a.group || "core", [...(m.get(a.group || "core") ?? []), a]);
    return Array.from(m.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [list]);
  const def = picked ? genericKind(picked) : null;
  const watch = useWatch(id, def?.ref, def?.ref.namespaced ? ns : null, { enabled: !!def });

  return (
    <div className="flex h-full min-h-0">
      <div className="flex w-72 shrink-0 flex-col border-r border-border bg-panel">
        <div className="flex flex-col gap-2 border-b border-border p-3">
          <Input icon={<Search className="size-3.5" />} placeholder="Filter kinds…" value={filter} onChange={(e) => setFilter(e.target.value)} />
          <div className="flex items-center justify-between">
            <Toggle checked={onlyCustom} onChange={setOnlyCustom} label="Custom only" />
            <Button
              size="icon-sm"
              variant="ghost"
              aria-label="Refresh discovery"
              onClick={async () => {
                qc.setQueryData(["api-resources", id], await api.listApiResources(id, true));
              }}
            >
              <RefreshCw className="size-3.5" />
            </Button>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-auto py-1">
          {apis.isLoading && <Spinner className="m-3" />}
          {groups.map(([g, items]) => (
            <div key={g} className="mb-1">
              <div className="truncate px-3 pt-2 pb-1 text-[11px] font-medium text-subtle">{g}</div>
              {items.map((a) => (
                <button
                  key={`${a.group}/${a.plural}`}
                  onClick={() => setPicked(a)}
                  className={cn(
                    "flex h-7 w-full items-center justify-between gap-2 px-3 text-left text-[12.5px]",
                    picked?.plural === a.plural && picked.group === a.group ? "bg-active font-medium" : "hover:bg-hover",
                  )}
                >
                  <span className="truncate">{a.kind}</span>
                  <span className="text-[11px] text-subtle">{a.version}</span>
                </button>
              ))}
            </div>
          ))}
        </div>
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        {!def ? (
          <EmptyState icon={<Puzzle className="size-5" />} title="Select a resource type" description="All API resources served by this cluster – including CustomResourceDefinitions and aggregated APIs – can be browsed and edited here." />
        ) : (
          <>
            <ViewHeader title={picked!.kind} description={`${picked!.plural}.${picked!.group || "core"}/${picked!.version} · ${picked!.namespaced ? "namespaced" : "cluster-scoped"}`} />
            <div className="min-h-0 flex-1">
              {!watch.ready ? (
                <Spinner className="m-4" />
              ) : watch.forbidden ? (
                <EmptyState title="Access denied" description={watch.error ?? undefined} />
              ) : (
                <ResourceTable
                  rows={watch.rows}
                  columns={def.columns}
                  onSelect={(r) => select({ clusterId: id, resource: def.ref, namespace: r.namespace, name: r.name })}
                  rowActions={(r) => <ActionMenu clusterId={id} def={def} row={r} />}
                  empty={<EmptyState title={`No ${picked!.kind} objects`} />}
                />
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
