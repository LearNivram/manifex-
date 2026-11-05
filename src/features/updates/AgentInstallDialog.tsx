import { CheckCircle2, Server } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { create } from "zustand";
import { Badge, Button, Dialog, ErrorBanner, Field, Input, Select } from "@/components/ui";
import { activateCluster } from "@/components/layout/ClusterSwitcher";
import { EnvBadge } from "@/features/resources/actions";
import { api, errorMessage } from "@/lib/ipc";
import type { AgentInstallResponse } from "@/lib/types";
import { useApp } from "@/store/app";

const useAgentDialog = create<{ clusterId: string | null; open: (id: string | null) => void }>((set) => ({
  clusterId: null,
  open: (clusterId) => set({ clusterId }),
}));

export function openAgentInstall(clusterId: string) {
  useAgentDialog.getState().open(clusterId);
}

const ROLES = [
  { value: "view", label: "Read-only (view) – recommended for viewers" },
  { value: "edit", label: "Edit – deploy, scale, restart, exec (no RBAC changes)" },
  { value: "admin", label: "Admin – full namespace administration" },
  { value: "cluster-admin", label: "Cluster admin – everything (use with care)" },
];

export function AgentInstallDialog() {
  const { clusterId, open } = useAgentDialog();
  const cluster = useApp((s) => s.clusters.find((c) => c.profile.id === clusterId));
  const [role, setRole] = useState("edit");
  const [exposure, setExposure] = useState("LoadBalancer");
  const [external, setExternal] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<AgentInstallResponse | null>(null);
  const queryClient = useQueryClient();
  if (!clusterId || !cluster) return null;
  const close = () => {
    open(null);
    setDone(null);
    setError(null);
  };

  const install = async () => {
    setBusy(true);
    setError(null);
    try {
      const res = await api.agentInstall(clusterId, { role, exposure, externalAddress: external.trim() || undefined });
      setDone(res);
      queryClient.invalidateQueries({ queryKey: ["agent-status", clusterId] });
      useApp.getState().setClusters(await api.listClusters());
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && close()}
      width={600}
      title="Install Manifex Agent"
      description="Deploys a small, authenticated gateway into the cluster. Manifex then connects through it – no kubeconfig or SSH needed afterwards."
      footer={
        done ? (
          <>
            <Button variant="ghost" onClick={close}>
              Close
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                close();
                activateCluster(done.profile.id);
              }}
            >
              Connect via agent
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" onClick={close}>
              Cancel
            </Button>
            <Button variant="primary" loading={busy} onClick={install}>
              {busy ? "Installing…" : "Install agent"}
            </Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-2 rounded-md border border-border bg-canvas px-3 py-2 text-[12.5px]">
          <Server className="size-4 text-muted" />
          <span className="text-muted">Install into</span>
          <span className="font-semibold">{cluster.profile.name}</span>
          <EnvBadge environment={cluster.profile.environment} />
        </div>
        {done ? (
          <div className="flex flex-col gap-2 rounded-md border border-ok/30 bg-ok-soft px-3 py-3 text-[12.5px]">
            <div className="flex items-center gap-2 font-medium text-ok">
              <CheckCircle2 className="size-4" /> Agent {done.result.version} is running
            </div>
            <div className="selectable text-fg">
              Endpoint <span className="font-mono">{done.result.endpoint}</span>
            </div>
            {done.result.alternativeEndpoints.length > 0 && (
              <div className="text-muted">
                Also reachable at: <span className="selectable font-mono">{done.result.alternativeEndpoints.join(", ")}</span>
              </div>
            )}
            <div className="text-fg">
              A new connection <Badge tone="accent">{done.profile.name}</Badge> was created with a private CA and an access token (stored encrypted on this machine).
            </div>
          </div>
        ) : (
          <>
            <Field label="Permissions of the agent" hint="Everything done through the agent is limited to this Kubernetes role.">
              <Select value={role} onChange={setRole} options={ROLES} />
            </Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label="Expose via" hint={exposure === "LoadBalancer" ? "Cloud / MetalLB / k3s ServiceLB address." : "A port (30000–32767) on every node."}>
                <Select
                  value={exposure}
                  onChange={setExposure}
                  options={[
                    { value: "LoadBalancer", label: "LoadBalancer" },
                    { value: "NodePort", label: "NodePort" },
                  ]}
                />
              </Field>
              <Field label="Public address (optional)" hint="DNS name or IP you reach the cluster with, if different.">
                <Input value={external} onChange={(e) => setExternal(e.target.value)} placeholder="k8s.example.com" />
              </Field>
            </div>
            <div className="rounded-md border border-border px-3 py-2 text-[12px] text-muted">
              Creates namespace <span className="font-mono">manifex-system</span> with a service account, role binding, service, TLS certificate (own CA) and
              access token, then waits until the agent is ready. Removable at any time from the Updates panel.
            </div>
            {error && <ErrorBanner message={error} />}
          </>
        )}
      </div>
    </Dialog>
  );
}
