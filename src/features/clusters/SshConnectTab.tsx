// "Via SSH server": the user enters host, user and SSH credentials; Manifex logs
// in, finds the kubeconfig on the server and tunnels the Kubernetes API.

import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, FileKey2, Fingerprint, KeyRound, ShieldAlert, ShieldCheck, Terminal, XCircle } from "lucide-react";
import { useState } from "react";
import { Badge, Button, ErrorBanner, Field, Input, Select, Textarea } from "@/components/ui";
import { activateCluster } from "@/components/layout/ClusterSwitcher";
import { api, errorMessage, type SshClusterRequest } from "@/lib/ipc";
import type { HostKeyInfo, SshKeyInfo, SshTestResult } from "@/lib/types";
import { useApp } from "@/store/app";

const ENVIRONMENTS = [
  { value: "development", label: "Development" },
  { value: "staging", label: "Staging" },
  { value: "production", label: "Production" },
  { value: "other", label: "Other" },
];

type KeyMode = "default" | "file" | "paste" | "password";

export function SshConnectTab({ onDone }: { onDone: () => void }) {
  const toast = useApp((s) => s.toast);
  const defaults = useQuery({ queryKey: ["ssh-default-keys"], queryFn: api.sshDefaultKeys });
  const [form, setForm] = useState({ name: "", host: "", port: "22", user: "", passphrase: "", password: "", keyPem: "", certificate: "", environment: "production" });
  const [keyMode, setKeyMode] = useState<KeyMode>("default");
  const [keyFile, setKeyFile] = useState<SshKeyInfo | null>(null);
  const [hostKey, setHostKey] = useState<HostKeyInfo | null>(null);
  const [trusted, setTrusted] = useState(false);
  const [result, setResult] = useState<SshTestResult | null>(null);
  const [context, setContext] = useState<string>("");
  const [busy, setBusy] = useState<"probe" | "test" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const set = (patch: Partial<typeof form>) => {
    setForm({ ...form, ...patch });
    setResult(null);
    if ("host" in patch || "port" in patch) {
      setHostKey(null);
      setTrusted(false);
    }
  };

  const defaultKey = defaults.data?.[0];
  const activeKey = keyMode === "file" ? keyFile : keyMode === "default" ? defaultKey : null;
  const needsPassphrase = !!activeKey?.encrypted;

  const request = (fingerprint: string): SshClusterRequest => ({
    name: form.name.trim() || form.host.trim(),
    host: form.host.trim(),
    port: Number(form.port) || 22,
    user: form.user.trim(),
    keyPath: keyMode === "file" ? keyFile?.path : undefined,
    keyPem: keyMode === "paste" ? form.keyPem : undefined,
    certificate: keyMode === "paste" && form.certificate.trim() ? form.certificate : undefined,
    passphrase: form.passphrase || undefined,
    password: form.password || undefined,
    hostKeyFingerprint: fingerprint,
    context: context || undefined,
    environment: form.environment,
  });

  const runTest = async (fingerprint: string) => {
    setBusy("test");
    setError(null);
    try {
      const r = await api.testSshCluster(request(fingerprint));
      setResult(r);
      if (r.ok && r.context && !context) setContext(r.context);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  /** Step 1: learn the host key. Known hosts (~/.ssh/known_hosts) are trusted automatically. */
  const connect = async () => {
    setBusy("probe");
    setError(null);
    setResult(null);
    try {
      const hk = await api.sshProbeHost(form.host.trim(), Number(form.port) || 22);
      setHostKey(hk);
      if (hk.knownHosts === "trusted") {
        setTrusted(true);
        await runTest(hk.fingerprint);
        return;
      }
      setTrusted(false);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy((b) => (b === "probe" ? null : b));
    }
  };

  const save = async () => {
    if (!hostKey) return;
    setBusy("save");
    setError(null);
    try {
      const p = await api.addSshCluster(request(hostKey.fingerprint));
      useApp.getState().setClusters(await api.listClusters());
      toast({ kind: "success", title: `Added ${p.name}`, message: "Connecting through SSH…" });
      onDone();
      activateCluster(p.id);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const credsOk =
    keyMode === "password" ? !!form.password : keyMode === "paste" ? form.keyPem.includes("PRIVATE KEY") : keyMode === "file" ? !!keyFile : !!defaultKey || !!form.password;
  const canConnect = form.host.trim() && form.user.trim() && credsOk && (!needsPassphrase || form.passphrase);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-start gap-3 rounded-md border border-border bg-canvas px-3 py-2.5 text-[12.5px] text-muted">
        <Terminal className="mt-0.5 size-4 shrink-0" />
        <span>
          For servers you normally reach with <span className="font-mono">ssh user@server</span> and run kubectl on. Manifex logs in, finds the kubeconfig on the
          server (kubectl, ~/.kube/config, k3s, RKE2, kubeadm, MicroK8s – with sudo if needed) and tunnels the Kubernetes API through SSH. Nothing has to be
          installed on the server.
        </span>
      </div>

      <div className="grid grid-cols-[1fr_96px_1fr] gap-3">
        <Field label="SSH server">
          <Input value={form.host} onChange={(e) => set({ host: e.target.value })} placeholder="server.example.com or 203.0.113.10" autoFocus />
        </Field>
        <Field label="Port">
          <Input value={form.port} onChange={(e) => set({ port: e.target.value })} inputMode="numeric" />
        </Field>
        <Field label="User">
          <Input value={form.user} onChange={(e) => set({ user: e.target.value })} placeholder="deploy" />
        </Field>
      </div>

      <Field label="Authentication">
        <Select
          value={keyMode}
          onChange={(v) => {
            setKeyMode(v as KeyMode);
            setResult(null);
          }}
          options={[
            { value: "default", label: defaultKey ? `Default SSH key (~/.ssh/${defaultKey.path.split(/[\\/]/).pop()})` : "Default SSH key (none found in ~/.ssh)" },
            { value: "file", label: "SSH key / certificate file…" },
            { value: "paste", label: "Paste private key (and certificate)" },
            { value: "password", label: "Password only" },
          ]}
        />
      </Field>

      {keyMode === "default" && defaultKey && (
        <KeySummary info={defaultKey} />
      )}
      {keyMode === "file" && (
        <div className="flex items-center gap-3">
          <Button
            icon={<FileKey2 className="size-4" />}
            onClick={async () => {
              try {
                const k = await api.pickSshKeyFile();
                if (k) {
                  setKeyFile(k);
                  setResult(null);
                }
              } catch (e) {
                setError(errorMessage(e));
              }
            }}
          >
            Choose key file…
          </Button>
          {keyFile ? <KeySummary info={keyFile} /> : <span className="text-[12.5px] text-muted">Select your private key; a matching <span className="font-mono">-cert.pub</span> certificate is used automatically.</span>}
        </div>
      )}
      {keyMode === "paste" && (
        <div className="grid grid-cols-2 gap-3">
          <Field label="Private key">
            <Textarea rows={5} value={form.keyPem} onChange={(e) => set({ keyPem: e.target.value })} placeholder="-----BEGIN OPENSSH PRIVATE KEY-----" />
          </Field>
          <Field label="SSH certificate (optional)">
            <Textarea rows={5} value={form.certificate} onChange={(e) => set({ certificate: e.target.value })} placeholder="ssh-ed25519-cert-v01@openssh.com AAAA…" />
          </Field>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {keyMode !== "password" && (
          <Field label="Key passphrase" hint={needsPassphrase ? "This key is encrypted." : "Only if your key is protected."}>
            <Input type="password" value={form.passphrase} onChange={(e) => set({ passphrase: e.target.value })} icon={<KeyRound className="size-3.5" />} />
          </Field>
        )}
        <Field label={keyMode === "password" ? "Password" : "User password (optional)"} hint="Used for password login and for sudo if the kubeconfig needs root.">
          <Input type="password" value={form.password} onChange={(e) => set({ password: e.target.value })} icon={<KeyRound className="size-3.5" />} />
        </Field>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <Field label="Name in Manifex">
          <Input value={form.name} onChange={(e) => set({ name: e.target.value })} placeholder={form.host || "production"} />
        </Field>
        <Field label="Environment">
          <Select value={form.environment} onChange={(v) => set({ environment: v })} options={ENVIRONMENTS} />
        </Field>
      </div>

      {hostKey && (
        <div
          className={`flex flex-col gap-2 rounded-md border px-3 py-2.5 text-[12.5px] ${
            hostKey.knownHosts === "changed" ? "border-err/40 bg-err-soft" : trusted ? "border-ok/30 bg-ok-soft" : "border-warn/40 bg-warn-soft"
          }`}
        >
          <div className="flex items-center gap-2 font-medium">
            {hostKey.knownHosts === "changed" ? (
              <ShieldAlert className="size-4 text-err" />
            ) : trusted ? (
              <ShieldCheck className="size-4 text-ok" />
            ) : (
              <Fingerprint className="size-4 text-warn" />
            )}
            {hostKey.knownHosts === "changed"
              ? "The server's host key does not match ~/.ssh/known_hosts – connection blocked."
              : hostKey.knownHosts === "trusted"
                ? "Server verified via ~/.ssh/known_hosts"
                : trusted
                  ? "Server key trusted"
                  : "First connection to this server – verify its fingerprint"}
          </div>
          <div className="selectable font-mono text-[12px]">
            {hostKey.algorithm} {hostKey.fingerprint}
          </div>
          {hostKey.knownHosts === "unknown" && !trusted && (
            <div className="flex items-center justify-between gap-3">
              <span className="text-muted">
                Compare with <span className="font-mono">ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub</span> on the server.
              </span>
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  setTrusted(true);
                  runTest(hostKey.fingerprint);
                }}
              >
                Trust & connect
              </Button>
            </div>
          )}
        </div>
      )}

      {result &&
        (result.ok ? (
          <div className="flex flex-col gap-2 rounded-md border border-ok/30 bg-ok-soft px-3 py-2.5 text-[12.5px] text-ok">
            <div className="flex items-center gap-2 font-medium">
              <CheckCircle2 className="size-4" /> Connected to Kubernetes {result.version} through SSH in {result.latencyMs} ms
            </div>
            <div className="text-fg">
              kubeconfig found via <Badge>{result.kubeconfigSource}</Badge>
              {result.identity ? (
                <>
                  {" "}
                  · identity <span className="font-medium">{result.identity}</span>
                </>
              ) : null}
            </div>
            {result.contexts.length > 1 && (
              <Field label="Context">
                <Select value={context || result.context || ""} onChange={setContext} options={result.contexts.map((c) => ({ value: c, label: c }))} />
              </Field>
            )}
          </div>
        ) : (
          <div className="selectable flex items-start gap-2 rounded-md border border-err/30 bg-err-soft px-3 py-2.5 text-[12.5px] text-err">
            <XCircle className="mt-0.5 size-4 shrink-0" />
            <span className="break-words">{result.message}</span>
          </div>
        ))}
      {error && <ErrorBanner message={error} />}

      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        {result?.ok ? (
          <Button variant="primary" loading={busy === "save"} onClick={save}>
            Save & connect
          </Button>
        ) : (
          <Button
            variant="primary"
            disabled={!canConnect || hostKey?.knownHosts === "changed"}
            loading={busy === "probe" || busy === "test"}
            onClick={() => (hostKey && trusted ? runTest(hostKey.fingerprint) : connect())}
          >
            {busy === "test" ? "Connecting…" : "Connect"}
          </Button>
        )}
      </div>
    </div>
  );
}

function KeySummary({ info }: { info: SshKeyInfo }) {
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-[12px]">
      <span className="selectable font-mono text-muted">{info.path}</span>
      <Badge>{info.algorithm}</Badge>
      {info.hasCertificate && <Badge tone="accent">certificate</Badge>}
      {info.encrypted && <Badge tone="warning">passphrase</Badge>}
    </div>
  );
}
