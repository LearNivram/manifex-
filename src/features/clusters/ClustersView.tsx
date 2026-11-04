import { Activity, ExternalLink, GitCompare, MoreHorizontal, Pencil, Plug, PlugZap, Plus, Star, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import {
  Badge,
  Button,
  Dialog,
  EmptyState,
  ErrorBanner,
  Field,
  Input,
  Menu,
  MenuContent,
  MenuItem,
  MenuSeparator,
  MenuTrigger,
  Select,
  StatusDot,
  Toggle,
} from "@/components/ui";
import { confirm } from "@/components/ui/confirm";
import { age, cn } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { ClusterView } from "@/lib/types";
import { activateCluster, clusterColor, statusHealth } from "@/components/layout/ClusterSwitcher";
import { EnvBadge } from "@/features/resources/actions";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useApp } from "@/store/app";

const SOURCE_LABEL: Record<string, string> = {
  kubeconfig: "kubeconfig",
  storedKubeconfig: "vault kubeconfig",
  token: "token",
  certificate: "certificate",
  agent: "agent",
  ssh: "via SSH",
};

async function refreshClusters() {
  useApp.getState().setClusters(await api.listClusters());
}

function EditDialog({ cluster, onClose }: { cluster: ClusterView; onClose: () => void }) {
  const p = cluster.profile;
  const [form, setForm] = useState({
    name: p.name,
    group: p.group ?? "",
    environment: p.environment,
    color: p.color ?? "",
    defaultNamespace: p.defaultNamespace ?? "",
    alertsEnabled: p.alertsEnabled,
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    setBusy(true);
    try {
      await api.updateCluster(p.id, {
        name: form.name,
        group: form.group || null,
        environment: form.environment,
        color: form.color || null,
        defaultNamespace: form.defaultNamespace || null,
        alertsEnabled: form.alertsEnabled,
      });
      await refreshClusters();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={`Edit ${p.name}`}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-4">
        <Field label="Name">
          <Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
        </Field>
        <Field label="Group">
          <Input value={form.group} onChange={(e) => setForm({ ...form, group: e.target.value })} placeholder="e.g. EU, Team A" />
        </Field>
        <Field label="Environment">
          <Select
            value={form.environment}
            onChange={(v) => setForm({ ...form, environment: v as typeof form.environment })}
            options={["development", "staging", "production", "other"].map((v) => ({ value: v, label: v[0].toUpperCase() + v.slice(1) }))}
          />
        </Field>
        <Field label="Accent colour" hint="Shown in the title bar">
          <div className="flex items-center gap-2">
            {["", "#4f5ff0", "#0ea5e9", "#14b8a6", "#a855f7", "#f97316", "#dc2626"].map((c) => (
              <button
                key={c || "auto"}
                onClick={() => setForm({ ...form, color: c })}
                className={cn("size-6 rounded-full border-2", form.color === c ? "border-fg" : "border-transparent")}
                style={{ background: c || "linear-gradient(135deg, var(--ok), var(--err))" }}
                aria-label={c || "Automatic"}
              />
            ))}
          </div>
        </Field>
        <Field label="Default namespace">
          <Input value={form.defaultNamespace} onChange={(e) => setForm({ ...form, defaultNamespace: e.target.value })} placeholder="all namespaces" />
        </Field>
        <div className="pt-6">
          <Toggle checked={form.alertsEnabled} onChange={(v) => setForm({ ...form, alertsEnabled: v })} label="Alerts & notifications" description="Watch warning events and node health" />
        </div>
      </div>
      {error && <ErrorBanner className="mt-4" message={error} />}
    </Dialog>
  );
}

function ClusterCard({ c, onEdit, compareSel, onCompareToggle }: { c: ClusterView; onEdit: () => void; compareSel: boolean; onCompareToggle: () => void }) {
  const toast = useApp((s) => s.toast);
  const active = useApp((s) => s.activeClusterId === c.profile.id);
  const [busy, setBusy] = useState(false);
  const st = c.status;
  const connected = st?.state === "connected" || st?.state === "reconnecting";

  const toggleConnection = async () => {
    setBusy(true);
    try {
      if (connected) {
        await api.disconnectCluster(c.profile.id);
      } else {
        await activateCluster(c.profile.id);
      }
      await refreshClusters();
    } finally {
      setBusy(false);
    }
  };
  const test = async () => {
    const r = await api.testCluster(c.profile.id).catch((e) => ({ ok: false, message: errorMessage(e), latencyMs: 0 }));
    toast(r.ok ? { kind: "success", title: `${c.profile.name} reachable`, message: `${(r as any).version} · ${r.latencyMs} ms` } : { kind: "error", title: `${c.profile.name} unreachable`, message: r.message ?? undefined });
  };
  const remove = async () => {
    const ok = await confirm({
      title: `Remove ${c.profile.name}?`,
      message: "The connection profile and any stored credentials are deleted from this machine. The cluster itself is not affected.",
      confirmLabel: "Remove",
      danger: true,
    });
    if (!ok) return;
    await api.removeCluster(c.profile.id);
    if (useApp.getState().activeClusterId === c.profile.id) useApp.getState().setActiveCluster(null);
    await refreshClusters();
  };

  return (
    <div
      className={cn("group flex flex-col gap-3 rounded-lg border bg-panel p-4 transition-colors", active ? "border-accent" : "border-border hover:border-border-strong")}
      onDoubleClick={() => activateCluster(c.profile.id)}
    >
      <div className="flex items-start gap-3">
        <span className="mt-1 h-8 w-1 shrink-0 rounded-full" style={{ background: clusterColor(c) }} />
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex items-center gap-2">
            <span className="truncate text-[14px] font-semibold">{c.profile.name}</span>
            <button
              onClick={async () => {
                await api.updateCluster(c.profile.id, { favorite: !c.profile.favorite });
                refreshClusters();
              }}
              aria-label="Favorite"
            >
              <Star className={cn("size-3.5", c.profile.favorite ? "fill-warn text-warn" : "text-subtle opacity-0 group-hover:opacity-100")} />
            </button>
          </div>
          <span className="truncate text-[12px] text-muted">{c.profile.source.type === "ssh" ? `${c.profile.source.user}@${c.profile.source.host}${c.profile.source.port !== 22 ? `:${c.profile.source.port}` : ""} · ${c.profile.source.kubeconfigSource ?? "ssh"}` : st?.server ?? ("server" in c.profile.source ? c.profile.source.server : "context" in c.profile.source ? c.profile.source.context : "")}</span>
        </div>
        <Menu>
          <MenuTrigger asChild>
            <Button size="icon-sm" variant="ghost" aria-label="Cluster actions">
              <MoreHorizontal className="size-4" />
            </Button>
          </MenuTrigger>
          <MenuContent>
            <MenuItem icon={<Pencil className="size-3.5" />} onSelect={onEdit}>
              Edit…
            </MenuItem>
            <MenuItem icon={<Activity className="size-3.5" />} onSelect={test}>
              Test connection
            </MenuItem>
            <MenuItem icon={<ExternalLink className="size-3.5" />} onSelect={() => api.openWindow(c.profile.id)}>
              Open in new window
            </MenuItem>
            <MenuItem icon={<GitCompare className="size-3.5" />} onSelect={onCompareToggle}>
              {compareSel ? "Remove from comparison" : "Select for comparison"}
            </MenuItem>
            <MenuSeparator />
            <MenuItem danger icon={<Trash2 className="size-3.5" />} onSelect={remove}>
              Remove…
            </MenuItem>
          </MenuContent>
        </Menu>
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        <EnvBadge environment={c.profile.environment} />
        <Badge>{SOURCE_LABEL[c.profile.source.type]}</Badge>
        {c.profile.group && <Badge tone="accent">{c.profile.group}</Badge>}
        {compareSel && <Badge tone="pending">compare</Badge>}
      </div>
      <div className="flex items-center justify-between gap-2 border-t border-border pt-3">
        <span className="flex min-w-0 items-center gap-2 text-[12px] text-muted">
          <StatusDot health={statusHealth(c)} pulse={st?.state === "connecting" || st?.state === "reconnecting"} />
          <span className="truncate">
            {st?.state === "connected" ? `${st.version} · ${st.latencyMs ?? "–"} ms` : st?.state === "error" ? st.message : st?.state ?? (c.profile.lastConnectedAt ? `last used ${age(c.profile.lastConnectedAt)} ago` : "never connected")}
          </span>
        </span>
        <Button size="sm" variant={connected ? "ghost" : "primary"} loading={busy} icon={connected ? <PlugZap className="size-3.5" /> : <Plug className="size-3.5" />} onClick={toggleConnection}>
          {connected ? "Disconnect" : "Connect"}
        </Button>
      </div>
    </div>
  );
}

export function ClustersView() {
  const clusters = useApp((s) => s.clusters);
  const setAddClusterOpen = useApp((s) => s.setAddClusterOpen);
  const navigate = useApp((s) => s.navigate);
  const [editing, setEditing] = useState<ClusterView | null>(null);
  const [filter, setFilter] = useState("");
  const [compare, setCompare] = useState<string[]>([]);
  const groups = useMemo(() => {
    const list = clusters.filter((c) => !filter || `${c.profile.name} ${c.profile.group ?? ""} ${c.profile.environment}`.toLowerCase().includes(filter.toLowerCase()));
    const m = new Map<string, ClusterView[]>();
    for (const c of list) {
      const g = c.profile.favorite ? "★ Favorites" : c.profile.group || "Ungrouped";
      m.set(g, [...(m.get(g) ?? []), c]);
    }
    return Array.from(m.entries()).sort(([a], [b]) => (a.startsWith("★") ? -1 : b.startsWith("★") ? 1 : a === "Ungrouped" ? 1 : b === "Ungrouped" ? -1 : a.localeCompare(b)));
  }, [clusters, filter]);

  return (
    <div className="flex h-full flex-col">
      <ViewHeader
        title="Clusters"
        description="Manage connections. Double-click a cluster to open it."
        actions={
          <>
            <Input placeholder="Filter clusters…" value={filter} onChange={(e) => setFilter(e.target.value)} className="w-56" />
            {compare.length === 2 && (
              <Button icon={<GitCompare className="size-4" />} onClick={() => navigate("compare", { left: compare[0], right: compare[1] })}>
                Compare
              </Button>
            )}
            <Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setAddClusterOpen(true)}>
              Add cluster
            </Button>
          </>
        }
      />
      <div className="min-h-0 flex-1 overflow-auto p-5">
        {clusters.length === 0 ? (
          <EmptyState
            icon={<Plug className="size-5" />}
            title="Connect your first cluster"
            description="Import contexts from your kubeconfig, or connect directly with a token, client certificate or agent."
            action={
              <Button variant="primary" icon={<Plus className="size-4" />} onClick={() => setAddClusterOpen(true)}>
                Add cluster
              </Button>
            }
          />
        ) : (
          <div className="flex flex-col gap-6">
            {groups.map(([g, list]) => (
              <section key={g} className="flex flex-col gap-3">
                <h2 className="text-[12px] font-semibold tracking-wide text-subtle uppercase">
                  {g} <span className="font-normal">· {list.length}</span>
                </h2>
                <div className="grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] gap-3">
                  {list.map((c) => (
                    <ClusterCard
                      key={c.profile.id}
                      c={c}
                      onEdit={() => setEditing(c)}
                      compareSel={compare.includes(c.profile.id)}
                      onCompareToggle={() =>
                        setCompare((cur) => (cur.includes(c.profile.id) ? cur.filter((x) => x !== c.profile.id) : [...cur, c.profile.id].slice(-2)))
                      }
                    />
                  ))}
                </div>
              </section>
            ))}
          </div>
        )}
      </div>
      {editing && <EditDialog cluster={editing} onClose={() => setEditing(null)} />}
    </div>
  );
}
