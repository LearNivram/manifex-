import { useQuery } from "@tanstack/react-query";
import { ArrowUpCircle, Download, Package, Plus, RefreshCw, RotateCcw, Search, Trash2, X } from "lucide-react";
import { lazy, Suspense, useMemo, useState } from "react";
import {
  Badge,
  Button,
  Dialog,
  EmptyState,
  ErrorBanner,
  Field,
  Input,
  Section,
  Spinner,
  StatusText,
  Tabs,
  Textarea,
  Toggle,
} from "@/components/ui";
import { confirm } from "@/components/ui/confirm";
import { age, cn, matchesFilter } from "@/lib/format";
import { api, errorMessage } from "@/lib/ipc";
import type { Health, HelmRelease } from "@/lib/types";
import { EnvBadge } from "@/features/resources/actions";
import { kindByRef, KINDS } from "@/features/resources/catalog";
import { ViewHeader } from "@/features/resources/ResourceView";
import { useActiveCluster, useApp, useNamespace } from "@/store/app";

const YamlViewer = lazy(() => import("@/features/editor/YamlViewer"));

const statusHealth = (s: string): Health =>
  s === "deployed" ? "ok" : s === "failed" ? "error" : s.startsWith("pending") ? "pending" : s === "superseded" || s === "uninstalled" ? "neutral" : "warning";

function UpgradeDialog({ release, onClose }: { release?: HelmRelease; onClose: () => void }) {
  const active = useActiveCluster()!;
  const ns = useNamespace();
  const toast = useApp((s) => s.toast);
  const [form, setForm] = useState({
    name: release?.name ?? "",
    namespace: release?.namespace ?? (ns || "default"),
    chart: "",
    version: release?.chartVersion ?? "",
    values: "",
    createNamespace: !release,
  });
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState<"dry" | "run" | "values" | null>(null);
  const [output, setOutput] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const search = useQuery({ queryKey: ["helm-search", query], queryFn: () => api.helmSearch(query), enabled: query.length > 1, retry: false });
  const detail = useQuery({
    queryKey: ["helm-release", active.profile.id, release?.namespace, release?.name],
    enabled: !!release,
    queryFn: () => api.helmRelease(active.profile.id, release!.namespace, release!.name),
  });
  useMemo(() => {
    if (detail.data && !form.values) setForm((f) => ({ ...f, values: detail.data!.values }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail.data]);

  const loadDefaults = async () => {
    setBusy("values");
    try {
      const v = await api.helmShowValues(form.chart, form.version || undefined);
      setForm((f) => ({ ...f, values: v }));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const run = async (dryRun: boolean) => {
    setBusy(dryRun ? "dry" : "run");
    setError(null);
    setOutput(null);
    try {
      const out = await api.helmUpgrade(active.profile.id, { ...form, dryRun, version: form.version || undefined });
      setOutput(out.stdout || out.stderr);
      if (!dryRun) {
        toast({ kind: "success", title: `${release ? "Upgraded" : "Installed"} ${form.name}` });
        onClose();
      }
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      width={820}
      title={release ? `Upgrade ${release.name}` : "Install chart"}
      description={
        <span className="flex items-center gap-2">
          Target cluster <b>{active.profile.name}</b> <EnvBadge environment={active.profile.environment} />
        </span>
      }
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button loading={busy === "dry"} disabled={!form.chart || !form.name || !!busy} onClick={() => run(true)}>
            Dry run
          </Button>
          <Button variant="primary" loading={busy === "run"} disabled={!form.chart || !form.name || !!busy} onClick={() => run(false)}>
            {release ? "Upgrade" : "Install"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        {!release && (
          <Field label="Search repositories" hint="Searches charts in the repositories configured for your Helm CLI.">
            <Input icon={<Search className="size-3.5" />} value={query} onChange={(e) => setQuery(e.target.value)} placeholder="nginx, postgresql, …" />
          </Field>
        )}
        {search.data && search.data.length > 0 && !form.chart && (
          <div className="max-h-40 overflow-auto rounded-md border border-border">
            {search.data.slice(0, 30).map((c) => (
              <button
                key={c.name}
                className="flex w-full items-center justify-between gap-3 border-b border-border px-3 py-1.5 text-left last:border-0 hover:bg-hover"
                onClick={() => setForm({ ...form, chart: c.name, version: c.version, name: form.name || c.name.split("/").pop()! })}
              >
                <span className="flex min-w-0 flex-col">
                  <span className="text-[13px] font-medium">{c.name}</span>
                  <span className="truncate text-[12px] text-muted">{c.description}</span>
                </span>
                <span className="tabular shrink-0 text-[12px] text-subtle">{c.version}</span>
              </button>
            ))}
          </div>
        )}
        <div className="grid grid-cols-2 gap-4">
          <Field label="Release name">
            <Input value={form.name} disabled={!!release} onChange={(e) => setForm({ ...form, name: e.target.value })} />
          </Field>
          <Field label="Namespace">
            <Input value={form.namespace} disabled={!!release} onChange={(e) => setForm({ ...form, namespace: e.target.value })} />
          </Field>
          <Field label="Chart" hint={release ? `Currently ${release.chart}-${release.chartVersion}. Use repo/chart or oci://…` : "repo/chart or oci://registry/chart"}>
            <Input value={form.chart} onChange={(e) => setForm({ ...form, chart: e.target.value })} placeholder={release ? `repo/${release.chart}` : "bitnami/nginx"} />
          </Field>
          <Field label="Version">
            <Input value={form.version} onChange={(e) => setForm({ ...form, version: e.target.value })} placeholder="latest" />
          </Field>
        </div>
        <Field label="Values (YAML)">
          <div className="flex flex-col gap-2">
            <Textarea rows={10} value={form.values} onChange={(e) => setForm({ ...form, values: e.target.value })} placeholder="# override chart values" />
            <div>
              <Button size="sm" variant="ghost" icon={<Download className="size-3.5" />} loading={busy === "values"} disabled={!form.chart} onClick={loadDefaults}>
                Load chart defaults
              </Button>
            </div>
          </div>
        </Field>
        {!release && <Toggle checked={form.createNamespace} onChange={(v) => setForm({ ...form, createNamespace: v })} label="Create namespace if missing" />}
        {error && <ErrorBanner message={error} />}
        {output && <pre className="selectable max-h-60 overflow-auto rounded-md border border-border bg-canvas p-3 font-mono text-[11.5px] whitespace-pre-wrap">{output}</pre>}
      </div>
    </Dialog>
  );
}

function ReleaseDetail({ release, onClose }: { release: HelmRelease; onClose: () => void }) {
  const active = useActiveCluster()!;
  const toast = useApp((s) => s.toast);
  const select = useApp((s) => s.select);
  const settings = useApp((s) => s.settings);
  const [tab, setTab] = useState<"history" | "values" | "manifest" | "notes" | "resources">("history");
  const [busy, setBusy] = useState<number | "uninstall" | null>(null);
  const [upgrade, setUpgrade] = useState(false);
  const q = useQuery({ queryKey: ["helm-release", active.profile.id, release.namespace, release.name], queryFn: () => api.helmRelease(active.profile.id, release.namespace, release.name) });

  const rollback = async (rev: number) => {
    if (!(await confirm({ title: `Roll back ${release.name} to revision ${rev}?`, message: `Cluster: ${active.profile.name}`, confirmLabel: "Roll back" }))) return;
    setBusy(rev);
    try {
      await api.helmRollback(active.profile.id, release.namespace, release.name, rev);
      toast({ kind: "success", title: `Rolled back ${release.name} to revision ${rev}` });
      q.refetch();
    } catch (e) {
      toast({ kind: "error", title: "Rollback failed", message: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  };
  const uninstall = async () => {
    const strict = active.profile.environment === "production" && (settings?.strictProductionConfirmations ?? true);
    const ok = await confirm({
      title: `Uninstall ${release.name}?`,
      message: `All resources of this release are deleted from ${active.profile.name} (${active.profile.environment}).`,
      confirmLabel: "Uninstall",
      danger: true,
      requireText: strict ? release.name : undefined,
    });
    if (!ok) return;
    const confirmName = strict ? release.name : undefined;
    setBusy("uninstall");
    try {
      await api.helmUninstall(active.profile.id, release.namespace, release.name, false, confirmName);
      toast({ kind: "success", title: `Uninstalled ${release.name}` });
      onClose();
    } catch (e) {
      toast({ kind: "error", title: "Uninstall failed", message: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  };

  return (
    <aside className="flex w-[520px] shrink-0 animate-slide-in flex-col border-l border-border bg-panel">
      <div className="flex flex-col gap-2 border-b border-border px-4 pt-3 pb-2">
        <div className="flex items-start justify-between">
          <div className="flex min-w-0 flex-col">
            <span className="text-[11.5px] text-subtle">Helm release</span>
            <span className="truncate text-[15px] font-semibold">{release.name}</span>
            <span className="mt-1 flex items-center gap-2 text-[12px] text-muted">
              <StatusText health={statusHealth(release.status)}>{release.status}</StatusText>
              <Badge>{release.namespace}</Badge>
              <span>
                {release.chart}-{release.chartVersion}
              </span>
            </span>
          </div>
          <Button size="icon-sm" variant="ghost" onClick={onClose} aria-label="Close">
            <X className="size-4" />
          </Button>
        </div>
        <div className="flex gap-1.5">
          <Button size="sm" icon={<ArrowUpCircle className="size-3.5" />} onClick={() => setUpgrade(true)}>
            Upgrade…
          </Button>
          <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} loading={busy === "uninstall"} onClick={uninstall}>
            Uninstall…
          </Button>
        </div>
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { id: "history", label: "History" },
            { id: "resources", label: "Resources" },
            { id: "values", label: "Values" },
            { id: "manifest", label: "Manifest" },
            { id: "notes", label: "Notes", hidden: !q.data?.notes },
          ]}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {q.isLoading && <Spinner className="m-4" />}
        {q.error && <ErrorBanner className="m-4" message={errorMessage(q.error)} />}
        {q.data && tab === "history" && (
          <div className="divide-y divide-border">
            {q.data.history.map((h) => (
              <div key={h.revision} className="flex items-center gap-3 px-4 py-2.5">
                <span className="tabular w-8 font-semibold">#{h.revision}</span>
                <div className="flex min-w-0 flex-1 flex-col">
                  <span className="flex items-center gap-2 text-[12.5px]">
                    <StatusText health={statusHealth(h.status)}>{h.status}</StatusText>
                    <span className="text-muted">
                      {h.chart}-{h.chartVersion}
                    </span>
                  </span>
                  <span className="truncate text-[12px] text-subtle">
                    {h.updated ? `${age(h.updated)} ago` : ""} {h.description ? `· ${h.description}` : ""}
                  </span>
                </div>
                {h.revision !== release.revision && (
                  <Button size="sm" icon={<RotateCcw className="size-3.5" />} loading={busy === h.revision} onClick={() => rollback(h.revision)}>
                    Roll back
                  </Button>
                )}
              </div>
            ))}
          </div>
        )}
        {q.data && tab === "resources" && (
          <div className="divide-y divide-border">
            {q.data.resources.map((r) => {
              const def = Object.values(KINDS).find((k) => k.ref.kind === r.kind);
              return (
                <button
                  key={`${r.kind}/${r.name}`}
                  disabled={!def}
                  className="flex w-full items-center justify-between gap-2 px-4 py-2 text-left text-[12.5px] hover:bg-hover disabled:hover:bg-transparent"
                  onClick={() => def && select({ clusterId: active.profile.id, resource: kindByRef(def.ref).ref, namespace: def.ref.namespaced ? r.namespace ?? release.namespace : null, name: r.name })}
                >
                  <span className="truncate font-medium">{r.name}</span>
                  <span className="text-muted">{r.kind}</span>
                </button>
              );
            })}
          </div>
        )}
        {q.data && (tab === "values" || tab === "manifest" || tab === "notes") && (
          <div className="h-full">
            <Suspense fallback={<Spinner />}>
              <YamlViewer
                value={tab === "values" ? q.data.values || "# no user-supplied values" : tab === "manifest" ? q.data.manifest : q.data.notes ?? ""}
                language={tab === "notes" ? "plaintext" : "yaml"}
              />
            </Suspense>
          </div>
        )}
      </div>
      {upgrade && <UpgradeDialog release={release} onClose={() => (setUpgrade(false), q.refetch())} />}
    </aside>
  );
}

function ReposDialog({ onClose }: { onClose: () => void }) {
  const repos = useQuery({ queryKey: ["helm-repos"], queryFn: api.helmRepos });
  const [name, setName] = useState("");
  const [url, setUrl] = useState("https://");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const act = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setError(null);
    try {
      await fn();
      repos.refetch();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title="Chart repositories" width={620}>
      <div className="flex flex-col gap-4">
        <div className="overflow-hidden rounded-md border border-border">
          {repos.data?.length === 0 && <div className="px-3 py-4 text-[12.5px] text-muted">No repositories configured.</div>}
          {repos.data?.map((r) => (
            <div key={r.name} className="flex items-center gap-3 border-b border-border px-3 py-2 last:border-0">
              <span className="font-medium">{r.name}</span>
              <span className="flex-1 truncate text-[12px] text-muted">{r.url}</span>
              <Button size="icon-sm" variant="ghost" loading={busy === r.name} onClick={() => act(r.name, () => api.helmRepoRemove(r.name))} aria-label="Remove repository">
                <Trash2 className="size-3.5" />
              </Button>
            </div>
          ))}
        </div>
        <div className="grid grid-cols-[1fr_2fr_auto] items-end gap-2">
          <Field label="Name">
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="bitnami" />
          </Field>
          <Field label="URL">
            <Input value={url} onChange={(e) => setUrl(e.target.value)} />
          </Field>
          <Button icon={<Plus className="size-4" />} loading={busy === "add"} disabled={!name || url.length < 10} onClick={() => act("add", () => api.helmRepoAdd(name, url))}>
            Add
          </Button>
        </div>
        <div>
          <Button size="sm" icon={<RefreshCw className="size-3.5" />} loading={busy === "update"} onClick={() => act("update", api.helmRepoUpdate)}>
            Update all repositories
          </Button>
        </div>
        {error && <ErrorBanner message={error} />}
      </div>
    </Dialog>
  );
}

export function HelmView() {
  const active = useActiveCluster()!;
  const ns = useNamespace();
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<HelmRelease | null>(null);
  const [install, setInstall] = useState(false);
  const [repos, setRepos] = useState(false);
  const status = useQuery({ queryKey: ["helm-status"], queryFn: api.helmStatus, staleTime: 60_000 });
  const releases = useQuery({ queryKey: ["helm-releases", active.profile.id, ns], queryFn: () => api.helmReleases(active.profile.id, ns || null), refetchInterval: 20_000 });
  const rows = useMemo(() => (releases.data ?? []).filter((r) => matchesFilter(`${r.name} ${r.namespace} ${r.chart} ${r.status}`, filter)), [releases.data, filter]);

  return (
    <div className="flex h-full">
      <div className="flex min-w-0 flex-1 flex-col">
        <ViewHeader
          title="Helm"
          description={status.data ? (status.data.available ? `Helm CLI ${status.data.version} · releases are read natively from the cluster` : status.data.message ?? "Helm CLI unavailable") : undefined}
          actions={
            <>
              <Input icon={<Search className="size-3.5" />} placeholder="Filter releases…" value={filter} onChange={(e) => setFilter(e.target.value)} className="w-56" />
              <Button disabled={!status.data?.available} onClick={() => setRepos(true)}>
                Repositories
              </Button>
              <Button variant="primary" icon={<Plus className="size-4" />} disabled={!status.data?.available} onClick={() => setInstall(true)}>
                Install chart
              </Button>
            </>
          }
        />
        <div className="min-h-0 flex-1 overflow-auto p-5">
          {status.data && !status.data.available && (
            <ErrorBanner className="mb-4" message={`Install, upgrade, rollback and uninstall require the Helm 3 CLI. ${status.data.message ?? ""}`} />
          )}
          {releases.error && <ErrorBanner className="mb-4" message={errorMessage(releases.error)} />}
          {releases.isLoading ? (
            <Spinner />
          ) : rows.length === 0 ? (
            <EmptyState icon={<Package className="size-5" />} title="No Helm releases" description={ns ? `No releases in namespace ${ns}.` : "No releases found in this cluster."} />
          ) : (
            <Section>
              <div className="grid grid-cols-[2fr_1.2fr_1.6fr_0.6fr_1fr_0.8fr] gap-3 border-b border-border px-4 py-2 text-[11.5px] font-medium text-muted">
                <span>Release</span>
                <span>Namespace</span>
                <span>Chart</span>
                <span>Revision</span>
                <span>Status</span>
                <span className="text-right">Updated</span>
              </div>
              {rows.map((r) => (
                <button
                  key={`${r.namespace}/${r.name}`}
                  onClick={() => setSelected(r)}
                  className={cn(
                    "grid w-full grid-cols-[2fr_1.2fr_1.6fr_0.6fr_1fr_0.8fr] items-center gap-3 border-b border-border/60 px-4 py-2 text-left text-[13px] last:border-0",
                    selected?.name === r.name && selected.namespace === r.namespace ? "bg-accent-soft" : "hover:bg-hover",
                  )}
                >
                  <span className="flex min-w-0 items-center gap-2 font-medium">
                    {r.icon ? <img src={r.icon} alt="" className="size-5 shrink-0 rounded object-contain" referrerPolicy="no-referrer" /> : <Package className="size-4 shrink-0 text-subtle" />}
                    <span className="truncate">{r.name}</span>
                  </span>
                  <span className="truncate text-muted">{r.namespace}</span>
                  <span className="truncate">
                    {r.chart}-{r.chartVersion}
                    {r.appVersion && <span className="ml-1 text-[12px] text-subtle">({r.appVersion})</span>}
                  </span>
                  <span className="tabular">{r.revision}</span>
                  <StatusText health={statusHealth(r.status)}>{r.status}</StatusText>
                  <span className="tabular text-right text-muted">{age(r.updated)}</span>
                </button>
              ))}
            </Section>
          )}
        </div>
      </div>
      {selected && <ReleaseDetail key={`${selected.namespace}/${selected.name}`} release={selected} onClose={() => (setSelected(null), releases.refetch())} />}
      {install && <UpgradeDialog onClose={() => (setInstall(false), releases.refetch())} />}
      {repos && <ReposDialog onClose={() => setRepos(false)} />}
    </div>
  );
}
