import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, FileUp, KeyRound, RefreshCw, XCircle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Badge, Button, Dialog, EmptyState, ErrorBanner, Field, Input, Select, Spinner, Tabs, Textarea, Toggle } from "@/components/ui";
import { cn } from "@/lib/format";
import { api, errorMessage, type DirectClusterRequest } from "@/lib/ipc";
import type { ConnectionTestResult, DiscoveredKubeconfig } from "@/lib/types";
import { useApp } from "@/store/app";

const ENVIRONMENTS = [
  { value: "development", label: "Development" },
  { value: "staging", label: "Staging" },
  { value: "production", label: "Production" },
  { value: "other", label: "Other" },
];

function KubeconfigTab({ onDone }: { onDone: () => void }) {
  const qc = useQueryClient();
  const toast = useApp((s) => s.toast);
  const discovered = useQuery({ queryKey: ["kubeconfigs"], queryFn: api.discoverKubeconfigs });
  const [extra, setExtra] = useState<DiscoveredKubeconfig[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [vault, setVault] = useState(false);
  const [environment, setEnvironment] = useState("development");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const files = useMemo(() => {
    const all = [...(discovered.data ?? []), ...extra];
    const seen = new Set<string>();
    return all.filter((f) => (seen.has(f.path) ? false : (seen.add(f.path), true)));
  }, [discovered.data, extra]);

  const key = (path: string, ctx: string) => `${path}\u0000${ctx}`;

  const pick = async () => {
    try {
      const f = await api.pickKubeconfigFile();
      if (f) {
        setExtra((e) => [...e, f]);
        setSelected((s) => new Set([...s, ...f.contexts.filter((c) => !c.alreadyAdded && !c.insecure).map((c) => key(f.path, c.name))]));
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const add = async () => {
    setBusy(true);
    setError(null);
    try {
      const entries = Array.from(selected).map((k) => {
        const [path, context] = k.split("\u0000");
        return { path, context, storeInVault: vault, environment };
      });
      const created = await api.addKubeconfigClusters(entries);
      await qc.invalidateQueries({ queryKey: ["kubeconfigs"] });
      useApp.getState().setClusters(await api.listClusters());
      toast({ kind: "success", title: `Added ${created.length} cluster${created.length === 1 ? "" : "s"}` });
      onDone();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <p className="text-[12.5px] text-muted">Contexts found in $KUBECONFIG and ~/.kube. Files are referenced, never copied – unless you choose the encrypted vault.</p>
        <div className="flex gap-2">
          <Button size="sm" variant="ghost" icon={<RefreshCw className="size-3.5" />} onClick={() => discovered.refetch()}>
            Rescan
          </Button>
          <Button size="sm" icon={<FileUp className="size-3.5" />} onClick={pick}>
            Import file…
          </Button>
        </div>
      </div>
      {discovered.isLoading && <Spinner />}
      {!discovered.isLoading && files.length === 0 && <EmptyState title="No kubeconfig found" description="Import a kubeconfig file or use a direct connection." />}
      <div className="flex max-h-[42vh] flex-col gap-3 overflow-auto">
        {files.map((f) => (
          <div key={f.path} className="rounded-lg border border-border">
            <div className="flex items-center justify-between border-b border-border bg-canvas px-3 py-2">
              <span className="selectable truncate font-mono text-[12px] text-muted">{f.path}</span>
              {f.error && <Badge tone="error">unreadable</Badge>}
            </div>
            {f.error && <div className="px-3 py-2 text-[12px] text-err">{f.error}</div>}
            {f.contexts.map((c) => {
              const k = key(f.path, c.name);
              const disabled = c.alreadyAdded || c.insecure;
              return (
                <label key={c.name} className={cn("flex items-center gap-3 border-b border-border px-3 py-2 last:border-0", disabled ? "opacity-60" : "cursor-pointer hover:bg-hover")}>
                  <input
                    type="checkbox"
                    className="accent-[var(--accent)]"
                    disabled={disabled}
                    checked={selected.has(k)}
                    onChange={() => {
                      const n = new Set(selected);
                      if (n.has(k)) n.delete(k);
                      else n.add(k);
                      setSelected(n);
                    }}
                  />
                  <div className="flex min-w-0 flex-1 flex-col">
                    <span className="flex items-center gap-2 text-[13px] font-medium">
                      {c.name}
                      {f.currentContext === c.name && <Badge tone="accent">current</Badge>}
                    </span>
                    <span className="truncate text-[12px] text-muted">
                      {c.server ?? "no server"} · {c.auth}
                      {c.namespace ? ` · ns ${c.namespace}` : ""}
                    </span>
                  </div>
                  {c.alreadyAdded && <Badge>added</Badge>}
                  {c.insecure && (
                    <Badge tone="error">
                      <AlertTriangle className="size-3" /> skips TLS verification
                    </Badge>
                  )}
                </label>
              );
            })}
          </div>
        ))}
      </div>
      <div className="grid grid-cols-2 gap-4">
        <Field label="Environment" hint="Production clusters require typed confirmations for destructive actions.">
          <Select value={environment} onChange={setEnvironment} options={ENVIRONMENTS} />
        </Field>
        <div className="pt-6">
          <Toggle checked={vault} onChange={setVault} label="Store copy in encrypted vault" description="Inlines certificates/tokens and encrypts them with a key held by the OS keychain." />
        </div>
      </div>
      {error && <ErrorBanner message={error} />}
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button variant="primary" disabled={!selected.size} loading={busy} onClick={add}>
          Add {selected.size || ""} {selected.size === 1 ? "cluster" : "clusters"}
        </Button>
      </div>
    </div>
  );
}

function TestResult({ result }: { result: ConnectionTestResult }) {
  return result.ok ? (
    <div className="flex items-start gap-2 rounded-md border border-ok/30 bg-ok-soft px-3 py-2 text-[12.5px] text-ok">
      <CheckCircle2 className="mt-0.5 size-4 shrink-0" />
      <span>
        Connected to Kubernetes {result.version} ({result.platform}) in {result.latencyMs} ms{result.identity ? ` as ${result.identity}` : ""}.
      </span>
    </div>
  ) : (
    <div className="selectable flex items-start gap-2 rounded-md border border-err/30 bg-err-soft px-3 py-2 text-[12.5px] text-err">
      <XCircle className="mt-0.5 size-4 shrink-0" />
      <span className="break-words">{result.message}</span>
    </div>
  );
}

function DirectTab({ onDone }: { onDone: () => void }) {
  const toast = useApp((s) => s.toast);
  const [form, setForm] = useState<DirectClusterRequest>({ name: "", kind: "token", server: "https://", environment: "development" });
  const [test, setTest] = useState<ConnectionTestResult | null>(null);
  const [busy, setBusy] = useState<"test" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<DirectClusterRequest>) => {
    setForm({ ...form, ...patch });
    setTest(null);
  };
  const runTest = async () => {
    setBusy("test");
    setError(null);
    try {
      setTest(await api.testDirectCluster(form));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  const save = async () => {
    setBusy("save");
    setError(null);
    try {
      const p = await api.addDirectCluster(form);
      useApp.getState().setClusters(await api.listClusters());
      toast({ kind: "success", title: `Added ${p.name}` });
      onDone();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };
  const valid = form.name.trim() && form.server.startsWith("https://") && form.server.length > 8 && (form.kind === "certificate" ? form.certPem && form.keyPem : form.token);

  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-2 gap-4">
        <Field label="Name">
          <Input value={form.name} onChange={(e) => set({ name: e.target.value })} placeholder="production-eu" autoFocus />
        </Field>
        <Field label="Environment">
          <Select value={form.environment ?? "development"} onChange={(v) => set({ environment: v })} options={ENVIRONMENTS} />
        </Field>
      </div>
      <Field label="Authentication">
        <Select
          value={form.kind}
          onChange={(v) => set({ kind: v as DirectClusterRequest["kind"] })}
          options={[
            { value: "token", label: "Bearer token (service account / OIDC token)" },
            { value: "certificate", label: "Client certificate" },
            { value: "agent", label: "Agent / authenticating gateway" },
          ]}
        />
      </Field>
      <Field label={form.kind === "agent" ? "Agent endpoint" : "API server URL"} hint="Only HTTPS endpoints are accepted.">
        <Input value={form.server} onChange={(e) => set({ server: e.target.value })} placeholder="https://10.0.0.1:6443" />
      </Field>
      {form.kind === "certificate" ? (
        <div className="grid grid-cols-2 gap-4">
          <Field label="Client certificate (PEM)">
            <Textarea rows={5} value={form.certPem ?? ""} onChange={(e) => set({ certPem: e.target.value })} placeholder="-----BEGIN CERTIFICATE-----" />
          </Field>
          <Field label="Client key (PEM)">
            <Textarea rows={5} value={form.keyPem ?? ""} onChange={(e) => set({ keyPem: e.target.value })} placeholder="-----BEGIN PRIVATE KEY-----" />
          </Field>
        </div>
      ) : (
        <Field label={form.kind === "agent" ? "Agent token" : "Bearer token"} hint="Stored encrypted; never shown again.">
          <Input type="password" value={form.token ?? ""} onChange={(e) => set({ token: e.target.value })} icon={<KeyRound className="size-3.5" />} />
        </Field>
      )}
      <Field label="Certificate authority (PEM)" hint="Required unless the server certificate is signed by a public CA. TLS verification is never disabled.">
        <Textarea rows={4} value={form.caPem ?? ""} onChange={(e) => set({ caPem: e.target.value })} placeholder="-----BEGIN CERTIFICATE-----" />
      </Field>
      <div className="grid grid-cols-2 gap-4">
        {form.kind !== "agent" && (
          <Field label="TLS server name (optional)">
            <Input value={form.tlsServerName ?? ""} onChange={(e) => set({ tlsServerName: e.target.value })} placeholder="kubernetes.default" />
          </Field>
        )}
        <Field label="Proxy (optional)" hint="http(s):// or socks5://">
          <Input value={form.proxyUrl ?? ""} onChange={(e) => set({ proxyUrl: e.target.value })} placeholder="socks5://bastion:1080" />
        </Field>
      </div>
      {test && <TestResult result={test} />}
      {error && <ErrorBanner message={error} />}
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button disabled={!valid} loading={busy === "test"} onClick={runTest}>
          Test connection
        </Button>
        <Button variant="primary" disabled={!valid} loading={busy === "save"} onClick={save}>
          Save cluster
        </Button>
      </div>
    </div>
  );
}

export function AddClusterDialog() {
  const open = useApp((s) => s.addClusterOpen);
  const setOpen = useApp((s) => s.setAddClusterOpen);
  const [tab, setTab] = useState<"kubeconfig" | "direct">("kubeconfig");
  useEffect(() => {
    if (open) setTab("kubeconfig");
  }, [open]);
  return (
    <Dialog open={open} onOpenChange={setOpen} title="Add cluster" description="Connect Manifex to Kubernetes clusters. Credentials stay on this machine, protected by the OS keychain." width={720}>
      <div className="flex flex-col gap-4">
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { id: "kubeconfig", label: "From kubeconfig" },
            { id: "direct", label: "Direct connection" },
          ]}
        />
        {open && (tab === "kubeconfig" ? <KubeconfigTab onDone={() => setOpen(false)} /> : <DirectTab onDone={() => setOpen(false)} />)}
      </div>
    </Dialog>
  );
}
