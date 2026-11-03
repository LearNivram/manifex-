import { Check, ChevronsUpDown, Plus, Server, Settings2, Star } from "lucide-react";
import { useMemo } from "react";
import { Menu, MenuContent, MenuItem, MenuLabel, MenuSeparator, MenuTrigger, StatusDot } from "@/components/ui";
import { cn } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { ClusterView, Health } from "@/lib/types";
import { useActiveCluster, useApp } from "@/store/app";

export function statusHealth(c?: ClusterView | null): Health {
  switch (c?.status?.state) {
    case "connected":
      return "ok";
    case "connecting":
      return "pending";
    case "reconnecting":
      return "warning";
    case "error":
      return "error";
    default:
      return "neutral";
  }
}

export const ENV_COLORS: Record<string, string> = {
  production: "var(--err)",
  staging: "var(--warn)",
  development: "var(--ok)",
  other: "var(--neutral)",
};

export function clusterColor(c?: ClusterView | null): string {
  return c?.profile.color || ENV_COLORS[c?.profile.environment ?? "other"];
}

export async function activateCluster(id: string) {
  const app = useApp.getState();
  app.setActiveCluster(id);
  const c = app.clusters.find((x) => x.profile.id === id);
  if (c?.status?.state !== "connected") {
    try {
      const status = await api.connectCluster(id);
      useApp.getState().updateStatus(status);
      if (useApp.getState().view === "clusters") useApp.getState().navigate("overview");
    } catch (e) {
      app.toast({ kind: "error", title: `Could not connect to ${c?.profile.name ?? "cluster"}`, message: errorMessage(e) });
    }
  }
}

export function ClusterSwitcher({ collapsed }: { collapsed?: boolean }) {
  const clusters = useApp((s) => s.clusters);
  const active = useActiveCluster();
  const navigate = useApp((s) => s.navigate);
  const setAddClusterOpen = useApp((s) => s.setAddClusterOpen);

  const grouped = useMemo(() => {
    const favs = clusters.filter((c) => c.profile.favorite);
    const groups = new Map<string, ClusterView[]>();
    for (const c of clusters.filter((c) => !c.profile.favorite)) {
      const g = c.profile.group || "Clusters";
      groups.set(g, [...(groups.get(g) ?? []), c]);
    }
    return { favs, groups: Array.from(groups.entries()).sort(([a], [b]) => (a === "Clusters" ? 1 : b === "Clusters" ? -1 : a.localeCompare(b))) };
  }, [clusters]);

  const item = (c: ClusterView) => (
    <MenuItem key={c.profile.id} onSelect={() => activateCluster(c.profile.id)} icon={<StatusDot health={statusHealth(c)} />}>
      <span className="flex items-center gap-2">
        <span className="truncate">{c.profile.name}</span>
        {c.profile.environment === "production" && <span className="text-[10.5px] font-semibold text-err uppercase">prod</span>}
        {c.profile.id === active?.profile.id && <Check className="size-3.5 text-accent" />}
      </span>
    </MenuItem>
  );

  return (
    <Menu>
      <MenuTrigger asChild>
        <button
          className={cn(
            "flex w-full items-center gap-2.5 rounded-lg border border-border bg-panel px-2.5 text-left transition-colors hover:border-border-strong",
            collapsed ? "h-9 justify-center px-0" : "h-12",
          )}
          aria-label="Switch cluster"
        >
          <span className="flex size-7 shrink-0 items-center justify-center rounded-md text-white" style={{ background: active ? clusterColor(active) : "var(--neutral)" }}>
            <Server className="size-3.5" />
          </span>
          {!collapsed && (
            <>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-[13px] font-semibold">{active?.profile.name ?? "No cluster selected"}</span>
                <span className="flex items-center gap-1.5 truncate text-[11.5px] text-muted">
                  <StatusDot health={statusHealth(active)} />
                  {active?.status?.state ?? "select a cluster"}
                  {active?.status?.version ? ` · ${active.status.version}` : ""}
                </span>
              </span>
              <ChevronsUpDown className="size-3.5 text-subtle" />
            </>
          )}
        </button>
      </MenuTrigger>
      <MenuContent align="start" className="max-h-[70vh] w-72 overflow-auto">
        {grouped.favs.length > 0 && (
          <>
            <MenuLabel>
              <span className="flex items-center gap-1">
                <Star className="size-3" /> Favorites
              </span>
            </MenuLabel>
            {grouped.favs.map(item)}
          </>
        )}
        {grouped.groups.map(([g, list]) => (
          <div key={g}>
            <MenuLabel>{g}</MenuLabel>
            {list.map(item)}
          </div>
        ))}
        {clusters.length > 0 && <MenuSeparator />}
        <MenuItem icon={<Plus className="size-3.5" />} onSelect={() => setAddClusterOpen(true)}>
          Add cluster…
        </MenuItem>
        <MenuItem icon={<Settings2 className="size-3.5" />} onSelect={() => navigate("clusters")}>
          Manage clusters
        </MenuItem>
      </MenuContent>
    </Menu>
  );
}
