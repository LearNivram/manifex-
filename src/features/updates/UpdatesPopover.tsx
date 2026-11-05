// Compact "Updates" area in the status bar: Manifex app updates and the
// manifex-agent version of every connected cluster – update with one click.

import * as Popover from "@radix-ui/react-popover";
import { useQueries, useQuery } from "@tanstack/react-query";
import { ArrowUpCircle, CheckCircle2, Download, PackagePlus, RefreshCw, Server, Trash2 } from "lucide-react";
import { useState } from "react";
import { Badge, Button, Spinner, StatusDot } from "@/components/ui";
import { confirm } from "@/components/ui/confirm";
import { useUpdater } from "@/hooks/useUpdater";
import { api, errorMessage } from "@/lib/ipc";
import { cn } from "@/lib/format";
import { useApp } from "@/store/app";
import { openAgentInstall } from "./AgentInstallDialog";

export function UpdatesPopover() {
  const updater = useUpdater();
  const clusters = useApp((s) => s.clusters);
  const settings = useApp((s) => s.settings);
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState<string | null>(null);
  const info = useQuery({ queryKey: ["app-info"], queryFn: api.appInfo, staleTime: Infinity });
  // Every connected cluster – including connections *through* an agent, so an agent
  // can be updated from the connection that uses it (it replaces itself without downtime).
  const connected = clusters.filter((c) => c.status?.state === "connected");
  const agents = useQueries({
    queries: connected.map((c) => ({
      queryKey: ["agent-status", c.profile.id],
      queryFn: () => api.agentStatus(c.profile.id),
      staleTime: 60_000,
      refetchInterval: 5 * 60_000,
      retry: false,
    })),
  });
  const agentUpdates = agents.filter((a) => a.data?.updateAvailable).length;
  const count = (updater.status === "available" ? 1 : 0) + agentUpdates;

  const updateAgent = async (clusterId: string, name: string) => {
    setBusy(clusterId);
    try {
      const st = await api.agentUpdate(clusterId);
      toast({ kind: "success", title: `Agent on ${name} updated to ${st.version}` });
      agents.forEach((a) => a.refetch());
    } catch (e) {
      toast({ kind: "error", title: "Agent update failed", message: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  };

  const removeAgent = async (clusterId: string, name: string, environment: string) => {
    const strict = environment === "production" && (settings?.strictProductionConfirmations ?? true);
    const ok = await confirm({
      title: `Remove the Manifex agent from ${name}?`,
      message: "Connections via this agent stop working. Your other connections are not affected.",
      confirmLabel: "Remove agent",
      danger: true,
      requireText: strict ? "manifex-agent" : undefined,
    });
    if (!ok) return;
    setBusy(clusterId);
    try {
      await api.agentUninstall(clusterId, strict ? "manifex-agent" : undefined);
      toast({ kind: "success", title: `Agent removed from ${name}` });
      agents.forEach((a) => a.refetch());
    } catch (e) {
      toast({ kind: "error", title: "Removal failed", message: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  };

  return (
    <Popover.Root onOpenChange={(o) => o && updater.status === "idle" && updater.check(true)}>
      <Popover.Trigger asChild>
        <button className={cn("flex items-center gap-1 hover:text-fg", count > 0 && "font-medium text-accent")} aria-label="Updates">
          {count > 0 ? <ArrowUpCircle className="size-3" /> : <CheckCircle2 className="size-3" />}
          {count > 0 ? `${count} update${count > 1 ? "s" : ""}` : `v${info.data?.version ?? ""}`}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content side="top" align="end" sideOffset={8} className="z-50 w-[380px] animate-pop-in rounded-xl border border-border bg-elevated p-3 shadow-pop">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-[13px] font-semibold">Updates</span>
            <Button size="icon-sm" variant="ghost" aria-label="Check again" onClick={() => {
              updater.check();
              agents.forEach((a) => a.refetch());
            }}>
              <RefreshCw className={cn("size-3.5", updater.status === "checking" && "animate-spin")} />
            </Button>
          </div>

          {/* Manifex app */}
          <div className="rounded-lg border border-border p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="flex flex-col">
                <span className="text-[12.5px] font-medium">Manifex app</span>
                <span className="text-[12px] text-muted">
                  Installed v{info.data?.version}
                  {updater.update ? ` · v${updater.update.version} available` : ""}
                </span>
              </div>
              {updater.status === "available" ? (
                <Button size="sm" variant="primary" icon={<Download className="size-3.5" />} onClick={updater.install}>
                  Update & restart
                </Button>
              ) : updater.status === "downloading" || updater.status === "ready" ? (
                <span className="flex items-center gap-2 text-[12px] text-muted">
                  <Spinner className="size-3.5" /> {updater.progress != null ? `${updater.progress}%` : "Installing…"}
                </span>
              ) : updater.status === "checking" ? (
                <Spinner className="size-3.5" />
              ) : updater.status === "none" ? (
                <Badge tone="ok">up to date</Badge>
              ) : null}
            </div>
            {updater.status === "downloading" && updater.progress != null && (
              <div className="mt-2 h-1 overflow-hidden rounded-full bg-hover">
                <div className="h-full bg-accent transition-[width]" style={{ width: `${updater.progress}%` }} />
              </div>
            )}
            {updater.update?.body && updater.status === "available" && (
              <div className="selectable mt-2 max-h-24 overflow-auto text-[12px] whitespace-pre-wrap text-muted">{updater.update.body}</div>
            )}
            {updater.status === "error" && <div className="mt-2 text-[12px] text-err">{updater.error}</div>}
          </div>

          {/* Agents */}
          <div className="mt-3 mb-1 text-[11px] font-medium tracking-wide text-subtle uppercase">Manifex agent per cluster</div>
          {connected.length === 0 && <div className="px-1 py-2 text-[12px] text-muted">Connect a cluster to manage its agent.</div>}
          <div className="flex flex-col gap-1.5">
            {connected.map((c, i) => {
              const q = agents[i];
              const st = q?.data;
              // Installing or removing the agent through itself makes no sense – that needs a direct connection.
              const viaAgent = c.profile.source.type === "agent";
              return (
                <div key={c.profile.id} className="flex items-center gap-2 rounded-lg border border-border px-3 py-2">
                  <Server className="size-3.5 shrink-0 text-muted" />
                  <div className="flex min-w-0 flex-1 flex-col">
                    <span className="truncate text-[12.5px] font-medium">{c.profile.name}</span>
                    <span className="flex items-center gap-1.5 text-[11.5px] text-muted">
                      {q?.isLoading ? (
                        "checking…"
                      ) : !st ? (
                        q?.error ? errorMessage(q.error) : "—"
                      ) : st.installed ? (
                        <>
                          <StatusDot health={st.healthy ? "ok" : "warning"} /> {[`v${st.version}`, st.role, st.exposure, viaAgent && "this connection"].filter(Boolean).join(" · ")}
                        </>
                      ) : (
                        "not installed"
                      )}
                    </span>
                  </div>
                  {st?.installed && st.updateAvailable && (
                    <Button size="sm" variant="primary" loading={busy === c.profile.id} onClick={() => updateAgent(c.profile.id, c.profile.name)}>
                      Update to {st.latestVersion}
                    </Button>
                  )}
                  {st?.installed && !st.updateAvailable && !viaAgent && (
                    <Button size="icon-sm" variant="ghost" aria-label="Remove agent" loading={busy === c.profile.id} onClick={() => removeAgent(c.profile.id, c.profile.name, c.profile.environment)}>
                      <Trash2 className="size-3.5" />
                    </Button>
                  )}
                  {st && !st.installed && !viaAgent && (
                    <Button size="sm" icon={<PackagePlus className="size-3.5" />} onClick={() => openAgentInstall(c.profile.id)}>
                      Install
                    </Button>
                  )}
                </div>
              );
            })}
          </div>
          <Popover.Arrow className="fill-[var(--elevated)]" />
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
