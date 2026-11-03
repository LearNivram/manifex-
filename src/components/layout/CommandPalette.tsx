import { Command } from "cmdk";
import {
  ArrowRight,
  Boxes,
  ExternalLink,
  FilePlus2,
  GitCompare,
  ListTree,
  Moon,
  Plus,
  Rocket,
  Ruler,
  ScrollText,
  Search,
  Server,
  Settings,
  SquareTerminal,
  Sun,
} from "lucide-react";
import React, { useEffect, useMemo, useState } from "react";
import * as RD from "@radix-ui/react-dialog";
import { Kbd, Spinner, StatusDot } from "@/components/ui";
import { useWatch } from "@/hooks/useWatch";
import { api, errorMessage } from "@/lib/ipc";
import type { Row, SearchHit } from "@/lib/types";
import { runAction } from "@/features/resources/actions";
import { KINDS } from "@/features/resources/catalog";
import { isConnected, useActiveCluster, useApp } from "@/store/app";
import { activateCluster, statusHealth } from "./ClusterSwitcher";
import { BOTTOM_NAV, NAV } from "./Sidebar";

function Item({ icon, children, onSelect, hint, value }: { icon?: React.ReactNode; children: React.ReactNode; onSelect: () => void; hint?: React.ReactNode; value?: string }) {
  return (
    <Command.Item
      value={value}
      onSelect={onSelect}
      className="flex h-9 cursor-default items-center gap-2.5 rounded-md px-2.5 text-[13px] text-fg aria-selected:bg-hover"
    >
      <span className="flex size-4 items-center justify-center text-muted">{icon}</span>
      <span className="flex min-w-0 flex-1 items-center gap-2 truncate">{children}</span>
      {hint && <span className="shrink-0 text-[11.5px] text-subtle">{hint}</span>}
    </Command.Item>
  );
}

function Group({ heading, children }: { heading: string; children: React.ReactNode }) {
  return (
    <Command.Group
      heading={heading}
      className="[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-subtle [&_[cmdk-group-heading]]:uppercase"
    >
      {children}
    </Command.Group>
  );
}

async function hitRow(hit: SearchHit): Promise<Row | undefined> {
  try {
    const rows = await api.listResources(hit.clusterId, hit.resource, hit.namespace, undefined, `metadata.name=${hit.name}`, 1);
    return rows[0];
  } catch {
    return undefined;
  }
}

export function CommandPalette() {
  const open = useApp((s) => s.paletteOpen);
  const mode = useApp((s) => s.paletteMode);
  const setPalette = useApp((s) => s.setPalette);
  const clusters = useApp((s) => s.clusters);
  const active = useActiveCluster();
  const navigate = useApp((s) => s.navigate);
  const settings = useApp((s) => s.settings);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const { rows: namespaces } = useWatch(active?.profile.id, KINDS.namespaces.ref, null, { enabled: open && isConnected(active) });

  useEffect(() => {
    if (!open) {
      setQuery("");
      setHits([]);
    }
  }, [open]);

  // Debounced cross-cluster search in the Rust core.
  useEffect(() => {
    const q = query.trim();
    if (q.length < 2 || !open) {
      setHits([]);
      return;
    }
    setSearching(true);
    const t = setTimeout(() => {
      api
        .globalSearch(q, 40)
        .then(setHits)
        .catch(() => setHits([]))
        .finally(() => setSearching(false));
    }, 220);
    return () => clearTimeout(t);
  }, [query, open]);

  const close = () => setPalette(false);
  const run = (fn: () => void) => () => {
    close();
    fn();
  };

  const openHit = async (hit: SearchHit, action?: "logs" | "shell" | "scale" | "restart") => {
    close();
    const app = useApp.getState();
    if (app.activeClusterId !== hit.clusterId) await activateCluster(hit.clusterId);
    if (!action) {
      app.select({ clusterId: hit.clusterId, resource: hit.resource, namespace: hit.namespace, name: hit.name });
      return;
    }
    const row = await hitRow(hit);
    try {
      await runAction(action, { clusterId: hit.clusterId, ref: hit.resource, namespace: hit.namespace, name: hit.name, row });
    } catch (e) {
      app.toast({ kind: "error", title: errorMessage(e) });
    }
  };

  const toggleTheme = async () => {
    if (!settings) return;
    const theme = document.documentElement.classList.contains("dark") ? "light" : "dark";
    try {
      useApp.getState().setSettings(await api.saveSettings({ ...settings, theme }));
    } catch {
      /* ignore */
    }
  };

  const navItems = useMemo(() => [...NAV.flatMap((s) => s.items), ...BOTTOM_NAV], []);
  const connected = isConnected(active);

  return (
    <RD.Root open={open} onOpenChange={(o) => setPalette(o)}>
      <RD.Portal>
        <RD.Overlay className="fixed inset-0 z-40 animate-fade-in bg-black/30" />
        <RD.Content className="fixed top-[14vh] left-1/2 z-50 w-[640px] max-w-[calc(100vw-48px)] -translate-x-1/2 animate-pop-in overflow-hidden rounded-xl border border-border bg-elevated shadow-pop focus:outline-none">
          <RD.Title className="sr-only">Command palette</RD.Title>
          <RD.Description className="sr-only">Search resources and run commands</RD.Description>
          <Command loop shouldFilter={true} className="flex flex-col">
            <div className="flex h-12 items-center gap-2.5 border-b border-border px-4">
              <Search className="size-4 text-subtle" />
              <Command.Input
                autoFocus
                value={query}
                onValueChange={setQuery}
                placeholder={mode === "search" ? "Search pods, deployments, services… across all connected clusters" : "Type a command or search…"}
                className="h-full flex-1 bg-transparent text-[14px] text-fg outline-none placeholder:text-subtle"
              />
              {searching && <Spinner />}
              <Kbd>esc</Kbd>
            </div>
            <Command.List className="max-h-[56vh] overflow-auto p-1.5">
              <Command.Empty className="px-3 py-8 text-center text-[13px] text-muted">{searching ? "Searching…" : "No results"}</Command.Empty>

              {hits.length > 0 && (
                <Group heading="Resources">
                  {hits.map((h) => {
                    const id = `${h.clusterId}/${h.resource.plural}/${h.namespace}/${h.name}`;
                    const label = (
                      <>
                        <span className="truncate font-medium">{h.name}</span>
                        <span className="truncate text-[12px] text-muted">
                          {h.resource.kind}
                          {h.namespace ? ` · ${h.namespace}` : ""} · {h.clusterName}
                        </span>
                      </>
                    );
                    return (
                      <React.Fragment key={id}>
                        <Item value={`open ${id} ${query}`} icon={<ArrowRight className="size-3.5" />} onSelect={() => openHit(h)}>
                          {label}
                        </Item>
                        {h.resource.kind === "Pod" && (
                          <>
                            <Item value={`logs ${id} ${query}`} icon={<ScrollText className="size-3.5" />} onSelect={() => openHit(h, "logs")} hint="Logs">
                              {label}
                            </Item>
                            <Item value={`shell ${id} ${query}`} icon={<SquareTerminal className="size-3.5" />} onSelect={() => openHit(h, "shell")} hint="Shell">
                              {label}
                            </Item>
                          </>
                        )}
                        {["Deployment", "StatefulSet"].includes(h.resource.kind) && (
                          <>
                            <Item value={`scale ${id} ${query}`} icon={<Ruler className="size-3.5" />} onSelect={() => openHit(h, "scale")} hint="Scale">
                              {label}
                            </Item>
                            <Item value={`restart ${id} ${query}`} icon={<Rocket className="size-3.5" />} onSelect={() => openHit(h, "restart")} hint="Restart">
                              {label}
                            </Item>
                            <Item value={`logs ${id} ${query}`} icon={<ScrollText className="size-3.5" />} onSelect={() => openHit(h, "logs")} hint="Logs">
                              {label}
                            </Item>
                          </>
                        )}
                      </React.Fragment>
                    );
                  })}
                </Group>
              )}

              <Group heading="Clusters">
                {clusters.map((c) => (
                  <Item key={c.profile.id} value={`switch cluster ${c.profile.name}`} icon={<StatusDot health={statusHealth(c)} />} onSelect={run(() => activateCluster(c.profile.id))} hint={c.profile.environment}>
                    Switch to {c.profile.name}
                  </Item>
                ))}
                <Item value="add cluster kubeconfig import" icon={<Plus className="size-3.5" />} onSelect={run(() => useApp.getState().setAddClusterOpen(true))}>
                  Add cluster…
                </Item>
                <Item value="compare clusters" icon={<GitCompare className="size-3.5" />} onSelect={run(() => navigate("compare"))}>
                  Compare clusters
                </Item>
              </Group>

              {connected && namespaces.length > 0 && (
                <Group heading="Namespaces">
                  <Item value="namespace all namespaces" icon={<ListTree className="size-3.5" />} onSelect={run(() => useApp.getState().setNamespace(active!.profile.id, ""))}>
                    All namespaces
                  </Item>
                  {namespaces.map((n) => (
                    <Item key={n.uid} value={`namespace ${n.name}`} icon={<ListTree className="size-3.5" />} onSelect={run(() => useApp.getState().setNamespace(active!.profile.id, n.name))}>
                      Switch namespace to {n.name}
                    </Item>
                  ))}
                </Group>
              )}

              <Group heading="Go to">
                {navItems.map((n) => (
                  <Item key={n.id} value={`go to ${n.label}`} icon={<n.icon className="size-3.5" />} onSelect={run(() => navigate(n.id))}>
                    {n.label}
                  </Item>
                ))}
              </Group>

              <Group heading="Actions">
                {connected && (
                  <Item value="create new resource yaml apply" icon={<FilePlus2 className="size-3.5" />} onSelect={run(() => useApp.getState().openEditor({ mode: "create", clusterId: active!.profile.id, namespace: useApp.getState().namespaces[active!.profile.id], initial: "" }))} hint="YAML">
                    Create resource…
                  </Item>
                )}
                {connected && (
                  <Item value="open terminal shell pod exec" icon={<SquareTerminal className="size-3.5" />} onSelect={run(() => navigate("pods"))}>
                    Open a shell – pick a pod…
                  </Item>
                )}
                <Item value="new window" icon={<ExternalLink className="size-3.5" />} onSelect={run(() => api.openWindow(active?.profile.id).catch(() => {}))}>
                  Open new window
                </Item>
                <Item value="toggle theme dark light" icon={document.documentElement.classList.contains("dark") ? <Sun className="size-3.5" /> : <Moon className="size-3.5" />} onSelect={run(toggleTheme)}>
                  Toggle dark / light mode
                </Item>
                <Item value="settings preferences" icon={<Settings className="size-3.5" />} onSelect={run(() => navigate("settings"))}>
                  Open settings
                </Item>
                <Item value="manage clusters" icon={<Boxes className="size-3.5" />} onSelect={run(() => navigate("clusters"))}>
                  Manage clusters
                </Item>
                <Item value="nodes" icon={<Server className="size-3.5" />} onSelect={run(() => navigate("nodes"))}>
                  Show nodes
                </Item>
              </Group>
            </Command.List>
          </Command>
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}
